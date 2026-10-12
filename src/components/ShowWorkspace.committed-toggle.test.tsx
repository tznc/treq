import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../../test/test-utils";
import userEvent from "@testing-library/user-event";
import { ShowWorkspace } from "./ShowWorkspace";
import type { JjLogCommit, JjLogResult, Workspace } from "../lib/api";

vi.mock("./FileBrowser", () => ({
  FileBrowser: () => <div data-testid="file-browser" />,
}));

vi.mock("./LinearCommitHistory", () => ({
  LinearCommitHistory: () => <div data-testid="linear-commit-history" />,
}));

let capturedProps: {
  showCommittedChanges?: boolean;
  onShowCommittedChangesChange?: (show: boolean) => void;
} | null = null;

vi.mock("./changes-diff-viewer/ChangesDiffViewerMain", () => ({
  ChangesDiffViewer: (props: {
    showCommittedChanges: boolean;
    onShowCommittedChangesChange?: (show: boolean) => void;
  }) => {
    capturedProps = props;
    return (
      <div
        data-testid="changes-viewer"
        data-show-committed={String(props.showCommittedChanges)}
      />
    );
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
    createSession: vi.fn().mockResolvedValue(42),
    ptyCreateSession: vi.fn().mockResolvedValue(undefined),
    ptyWrite: vi.fn().mockResolvedValue(undefined),
    checkAndRebaseWorkspaces: vi.fn().mockResolvedValue({
      rebased: false,
      success: true,
      has_conflicts: false,
      conflicted_files: [],
      message: "No rebase needed",
      bookmark_conflicts: [],
    }),
    resolveBookmarkConflict: vi.fn().mockResolvedValue({
      success: true,
      message: "Resolved",
    }),
    listCommits: vi.fn(),
  };
});

const workingCopyCommit: JjLogCommit = {
  commit_id: "abc123",
  short_id: "abc",
  change_id: "chg123",
  description: "(no description)",
  author_name: "Test User",
  timestamp: new Date().toISOString(),
  parent_ids: [],
  is_working_copy: true,
  bookmarks: [],
  is_immutable: false,
  insertions: 0,
  deletions: 0,
  on_target_only: false,
};

const realWorkspaceCommit: JjLogCommit = {
  commit_id: "def456",
  short_id: "def",
  change_id: "chg456",
  description: "Add feature",
  author_name: "Test User",
  timestamp: new Date().toISOString(),
  parent_ids: [],
  is_working_copy: false,
  bookmarks: [],
  is_immutable: false,
  insertions: 5,
  deletions: 2,
  on_target_only: false,
};

function makeLogResult(commits: JjLogCommit[]): JjLogResult {
  return {
    commits,
    target_branch: "main",
    workspace_branch: "feature-one",
  };
}

const workspace: Workspace = {
  id: 7,
  repo_path: "/Users/test/repo",
  workspace_name: "feature-one",
  workspace_path: "/Users/test/repo/.treq/workspaces/feature-one",
  branch_name: "feature-one",
  title: "feature-one",
  created_at: new Date().toISOString(),
  not_on_remote: false,
};

function renderWorkspace() {
  return render(
    <ShowWorkspace
      repositoryPath={workspace.repo_path}
      workspace={workspace}
      mainRepoBranch="main"
      initialSelectedFile={null}
      onDeleteWorkspace={vi.fn()}
    />,
  );
}

const defaultBranchWorkspace: Workspace = {
  ...workspace,
  id: 9,
  workspace_name: "main",
  branch_name: "main",
};

function renderDefaultBranchWorkspace() {
  return render(
    <ShowWorkspace
      repositoryPath={defaultBranchWorkspace.repo_path}
      workspace={defaultBranchWorkspace}
      mainRepoBranch="main"
      initialSelectedFile={null}
      onDeleteWorkspace={vi.fn()}
    />,
  );
}

describe("Committed Show toggle wiring", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    capturedProps = null;
  });

  it("passes showCommittedChanges=false without a toggle callback for a default-branch workspace", async () => {
    const { listCommits } = await import("../lib/api");
    vi.mocked(listCommits).mockResolvedValue(
      makeLogResult([workingCopyCommit, realWorkspaceCommit]),
    );

    renderDefaultBranchWorkspace();

    const user = userEvent.setup();
    const changesTab = await screen.findByRole("tab", { name: /changes/i });
    await user.click(changesTab);

    const viewer = await screen.findByTestId("changes-viewer");
    await waitFor(() => {
      expect(viewer.dataset.showCommitted).toBe("false");
    });
    expect(capturedProps?.onShowCommittedChangesChange).toBeUndefined();
  });

  it("does not render a Review-toolbar Committed toggle for a default-branch workspace", async () => {
    const { listCommits } = await import("../lib/api");
    vi.mocked(listCommits).mockResolvedValue(
      makeLogResult([workingCopyCommit, realWorkspaceCommit]),
    );

    renderDefaultBranchWorkspace();

    const user = userEvent.setup();
    const changesTab = await screen.findByRole("tab", { name: /changes/i });
    await user.click(changesTab);

    await screen.findByTestId("changes-viewer");
    expect(
      screen.queryByRole("button", { name: /^Committed$/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^Show$/ }),
    ).not.toBeInTheDocument();
  });

  it("passes showCommittedChanges and a toggle callback for a feature-branch workspace", async () => {
    const { listCommits } = await import("../lib/api");
    vi.mocked(listCommits).mockResolvedValue(
      makeLogResult([workingCopyCommit, realWorkspaceCommit]),
    );

    renderWorkspace();

    const user = userEvent.setup();
    const changesTab = await screen.findByRole("tab", { name: /changes/i });
    await user.click(changesTab);

    await screen.findByTestId("changes-viewer");
    await waitFor(() => {
      expect(capturedProps?.showCommittedChanges).toBe(true);
      expect(typeof capturedProps?.onShowCommittedChangesChange).toBe(
        "function",
      );
    });
  });
});
