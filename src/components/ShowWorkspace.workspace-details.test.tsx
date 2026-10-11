import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor } from "../../test/test-utils";
import { ShowWorkspace } from "./ShowWorkspace";
import type { PromptHistoryEntry, Workspace } from "../lib/api";

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
    listCommits: vi.fn().mockResolvedValue({
      commits: [],
      target_branch: "main",
      workspace_branch: "feature-one",
    }),
    getWorkspaceStartingPrompt: vi.fn(),
  };
});

const workspace: Workspace = {
  id: 7,
  repo_path: "/Users/test/repo",
  workspace_name: "feature-one",
  workspace_path: "/Users/test/repo/.treq/workspaces/feature-one",
  branch_name: "feature-one",
  title: "feature-one",
  created_at: "2026-01-01T00:00:00.000Z",
  not_on_remote: false,
};

function renderWorkspace(
  onViewFullPrompt?: (promptId: number) => void,
  target: Workspace = workspace,
) {
  return render(
    <ShowWorkspace
      repositoryPath={target.repo_path}
      workspace={target}
      mainRepoBranch="main"
      initialSelectedFile={null}
      onDeleteWorkspace={vi.fn()}
      onSessionCreated={vi.fn()}
      onViewFullPrompt={onViewFullPrompt}
    />,
  );
}

describe("Workspace details popover", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the workspace's starting prompt when one was recorded", async () => {
    const api = await import("../lib/api");
    const entry: PromptHistoryEntry = {
      id: 1,
      workspace_id: workspace.id,
      session_id: 42,
      prompt_text: "Build the login page",
      agent: "claude",
      created_at: "2026-01-01T00:00:00.000Z",
      workspace_label: "feature-one",
    };
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(entry);

    renderWorkspace();

    const user = userEvent.setup();
    const detailsButton = await screen.findByTestId("workspace-details-button");
    await user.click(detailsButton);

    await waitFor(() =>
      expect(api.getWorkspaceStartingPrompt).toHaveBeenCalledWith(
        workspace.repo_path,
        workspace.id,
      ),
    );

    expect(
      await screen.findByTestId("workspace-starting-prompt-text"),
    ).toHaveTextContent("Build the login page");
  });

  it("shows a fallback message when no prompt was recorded", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(null);

    renderWorkspace();

    const user = userEvent.setup();
    const detailsButton = await screen.findByTestId("workspace-details-button");
    await user.click(detailsButton);

    expect(
      await screen.findByText("No prompt recorded for this workspace yet."),
    ).toBeTruthy();
  });

  it("copies the starting prompt to the clipboard", async () => {
    const api = await import("../lib/api");
    const entry: PromptHistoryEntry = {
      id: 1,
      workspace_id: workspace.id,
      session_id: 42,
      prompt_text: "Build the login page",
      agent: "claude",
      created_at: "2026-01-01T00:00:00.000Z",
      workspace_label: "feature-one",
    };
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(entry);

    renderWorkspace();

    const user = userEvent.setup();
    const detailsButton = await screen.findByTestId("workspace-details-button");
    await user.click(detailsButton);
    await screen.findByTestId("workspace-starting-prompt-text");

    const writeTextSpy = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue(undefined);

    const copyButton = await screen.findByRole("button", { name: /copy/i });
    await user.click(copyButton);

    expect(writeTextSpy).toHaveBeenCalledWith("Build the login page");
  });

  it("closes the popover and calls onViewFullPrompt with the starting prompt's id", async () => {
    const api = await import("../lib/api");
    const entry: PromptHistoryEntry = {
      id: 99,
      workspace_id: workspace.id,
      session_id: 42,
      prompt_text: "Build the login page",
      agent: "claude",
      created_at: "2026-01-01T00:00:00.000Z",
      workspace_label: "feature-one",
    };
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(entry);

    const onViewFullPrompt = vi.fn();
    renderWorkspace(onViewFullPrompt);

    const user = userEvent.setup();
    const detailsButton = await screen.findByTestId("workspace-details-button");
    await user.click(detailsButton);
    await screen.findByTestId("workspace-starting-prompt-text");

    await user.click(await screen.findByTestId("view-full-prompt-button"));

    expect(onViewFullPrompt).toHaveBeenCalledWith(99);
    await waitFor(() =>
      expect(
        screen.queryByTestId("workspace-details-popover"),
      ).not.toBeInTheDocument(),
    );
  });
});

describe("Tracker item badge", () => {
  it("links the workspace header to the Jira issue it was kicked off from", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(null);
    renderWorkspace(undefined, {
      ...workspace,
      metadata: JSON.stringify({
        tracker_provider: "jira",
        tracker_item_key: "ENG-42",
        tracker_item_url: "https://acme.atlassian.net/browse/ENG-42",
        tracker_item_title: "Add Jira integration",
      }),
    });

    const badge = await screen.findByTestId("tracker-item-badge");
    expect(badge).toHaveTextContent("ENG-42");
    expect(badge).toHaveAttribute("title", "Add Jira integration");
  });

  it("ignores metadata from an unknown tracker", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getWorkspaceStartingPrompt).mockResolvedValue(null);
    renderWorkspace(undefined, {
      ...workspace,
      metadata: JSON.stringify({
        tracker_provider: "asana",
        tracker_item_key: "A-1",
        tracker_item_url: "https://app.asana.com/0/1",
      }),
    });

    await screen.findByTestId("workspace-details-button");
    expect(screen.queryByTestId("tracker-item-badge")).toBeNull();
  });
});
