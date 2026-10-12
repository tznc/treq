import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../../test/test-utils";
import userEvent from "@testing-library/user-event";
import { ShowWorkspace } from "./ShowWorkspace";
import * as api from "../lib/api";
import {
  createMockWorkspace,
  createMockWorkspaceStatus,
} from "../../test/factories/workspace.factory";

vi.mock("./FileBrowser", () => ({
  FileBrowser: () => <div data-testid="file-browser" />,
}));

vi.mock("./LinearCommitHistory", () => ({
  LinearCommitHistory: () => <div data-testid="linear-commit-history" />,
}));

vi.mock("./changes-diff-viewer/ChangesDiffViewerMain", () => ({
  ChangesDiffViewer: () => <div data-testid="changes-viewer" />,
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
    lsWorkspaceWithStatus: vi.fn().mockResolvedValue([]),
    getWorkspaceReadme: vi.fn().mockResolvedValue(null),
    jjGetDefaultBranch: vi.fn().mockResolvedValue("main"),
    listConflictedFiles: vi.fn().mockResolvedValue([]),
    jjGetBranches: vi.fn().mockResolvedValue([]),
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
    }),
    pullWorkspaceFromRemote: vi.fn(),
    pushWorkspaceToRemote: vi.fn(),
    getWorkspaceStatus: vi.fn(),
  };
});

const workspace = createMockWorkspace({
  id: 7,
  branch_name: "feature-one",
  workspace_name: "feature-one",
  workspace_path: "/Users/test/repo/.treq/workspaces/feature-one",
});

describe("ShowWorkspace Sync with remote conflicts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("still pushes when pull leaves conflicts, then refreshes status", async () => {
    const user = userEvent.setup();

    vi.mocked(api.pullWorkspaceFromRemote).mockResolvedValue({
      success: true,
      message:
        "Rebased 1 local commit(s) onto remote tip with conflicts to resolve",
      was_diverged: true,
      commits_rebased: 1,
      has_conflicts: true,
    });
    vi.mocked(api.pushWorkspaceToRemote).mockResolvedValue("pushed");

    vi.mocked(api.getWorkspaceStatus)
      .mockResolvedValueOnce(
        createMockWorkspaceStatus({
          current: workspace,
          remote_sync: { type: "Diverged", data: { ahead: 1, behind: 1 } },
          conflicted_files: [],
          has_conflicts: false,
        }),
      )
      .mockResolvedValue(
        createMockWorkspaceStatus({
          current: workspace,
          remote_sync: { type: "Ahead", data: { count: 1 } },
          conflicted_files: ["shared.txt"],
          has_conflicts: true,
        }),
      );

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        initialSelectedFile={null}
        onSessionCreated={vi.fn()}
        onDeleteWorkspace={vi.fn()}
      />,
    );

    const syncButton = await screen.findByRole("button", {
      name: /↓\s*1\s*↑\s*1/,
    });
    await user.click(syncButton);

    await waitFor(() => {
      expect(api.pullWorkspaceFromRemote).toHaveBeenCalledWith(
        workspace.repo_path,
        workspace.id,
      );
      expect(api.pushWorkspaceToRemote).toHaveBeenCalledWith(
        workspace.repo_path,
        workspace.id,
      );
    });

    await waitFor(() => {
      expect(screen.getByText(/1\s+conflict/i)).toBeTruthy();
    });
  });
});
