import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor } from "../../test/test-utils";
import { ShowWorkspace } from "./ShowWorkspace";
import type { Workspace } from "../lib/api";
import type { SessionCreationInfo } from "../types/sessions";
import type { TerminalSessionSummary } from "./terminal/types";

// Capture the onCreateAgentWithReview callback so tests can invoke it directly.
let capturedOnCreateAgentWithReview:
  | ((
      review: string,
      mode: "plan" | "acceptEdits",
      useIdleAgent?: boolean,
    ) => Promise<void>)
  | undefined;
let capturedOnStartAgentReview:
  | ((prompt: string, agent?: string) => Promise<void>)
  | undefined;

vi.mock("./FileBrowser", () => ({
  FileBrowser: () => <div data-testid="file-browser" />,
}));

vi.mock("./LinearCommitHistory", () => ({
  LinearCommitHistory: () => <div data-testid="linear-commit-history" />,
}));

vi.mock("./changes-diff-viewer/ChangesDiffViewerMain", () => ({
  ChangesDiffViewer: ({
    onCreateAgentWithReview,
    onStartAgentReview,
  }: {
    onCreateAgentWithReview?: (
      review: string,
      mode: "plan" | "acceptEdits",
      useIdleAgent?: boolean,
    ) => Promise<void>;
    onStartAgentReview?: (prompt: string, agent?: string) => Promise<void>;
  }) => {
    capturedOnCreateAgentWithReview = onCreateAgentWithReview;
    capturedOnStartAgentReview = onStartAgentReview;
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
    listCommits: vi.fn().mockResolvedValue({
      commits: [],
      target_branch: "main",
      workspace_branch: "feature-one",
    }),
  };
});

const workspace: Workspace = {
  id: 7,
  repo_path: "/Users/test/repo",
  workspace_name: "feature-one",
  workspace_path: "feature-one",
  branch_name: "feature-one",
  title: "feature-one",
  created_at: new Date().toISOString(),
  not_on_remote: false,
};

const IDLE_AGENT: TerminalSessionSummary = {
  id: "agent-84",
  kind: "agent",
  name: "Claude 1",
  branchName: "feature-one",
  isMainRepo: false,
  isStreaming: false,
};

function renderWorkspace(
  onSessionCreated: (s: SessionCreationInfo) => void,
  options?: {
    idleAgentSession?: TerminalSessionSummary;
    onSendToIdleAgent?: (sessionId: number, prompt: string) => void;
  },
) {
  capturedOnCreateAgentWithReview = undefined;
  capturedOnStartAgentReview = undefined;
  return render(
    <ShowWorkspace
      repositoryPath={workspace.repo_path}
      workspace={workspace}
      mainRepoBranch="main"
      initialSelectedFile={null}
      onDeleteWorkspace={vi.fn()}
      onSessionCreated={onSessionCreated}
      idleAgentSession={options?.idleAgentSession}
      onSendToIdleAgent={options?.onSendToIdleAgent}
    />,
  );
}

async function openReviewTab() {
  const user = userEvent.setup();
  const reviewTab = await screen.findByRole("tab", { name: /changes/i });
  await user.click(reviewTab);
  await screen.findByTestId("changes-viewer");
  await waitFor(() => expect(capturedOnCreateAgentWithReview).toBeDefined());
}

describe("Send review to terminal respects default agent setting", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    capturedOnCreateAgentWithReview = undefined;
  });

  it("passes agent=undefined when no default agent is configured", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue(null);
    vi.mocked(api.getSetting).mockResolvedValue(null);

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "plan");

    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.agent).toBeUndefined();
  });

  it("passes agent=codex when repo default_agent is codex", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue("codex");
    vi.mocked(api.getSetting).mockResolvedValue(null);

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "plan");

    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.agent).toBe("codex");
  });

  it("passes agent=cursor when repo default_agent is cursor", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue("cursor");
    vi.mocked(api.getSetting).mockResolvedValue(null);

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "acceptEdits");

    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.agent).toBe("cursor");
  });

  it("repo setting takes precedence over app-level default_agent", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue("codex");
    vi.mocked(api.getSetting).mockResolvedValue("cursor");

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "plan");

    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.agent).toBe("codex");
  });

  it("falls back to app-level default_agent when no repo setting", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue(null);
    vi.mocked(api.getSetting).mockResolvedValue("codex");

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "plan");

    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.agent).toBe("codex");
  });

  it("includes the review markdown in the pending prompt", async () => {
    const api = await import("../lib/api");
    vi.mocked(api.getRepoSetting).mockResolvedValue("codex");
    vi.mocked(api.getSetting).mockResolvedValue(null);

    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    const reviewText = "## Code Review\n\nPlease fix the bug.";
    await capturedOnCreateAgentWithReview!(reviewText, "plan");

    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.pendingPrompt).toBe(reviewText);
    expect(sessionInfo.permissionMode).toBe("plan");
  });

  it("identifies the review agent workspace without passing its path", async () => {
    const onSessionCreated = vi.fn();
    renderWorkspace(onSessionCreated);

    await openReviewTab();

    await capturedOnCreateAgentWithReview!("review text", "plan");

    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.workspaceId).toBe(7);
    expect(sessionInfo.workspacePath).toBeNull();
  });

  it("sends Edit to an existing idle agent when the user opts in", async () => {
    const onSessionCreated = vi.fn();
    const onSendToIdleAgent = vi.fn();
    renderWorkspace(onSessionCreated, {
      idleAgentSession: IDLE_AGENT,
      onSendToIdleAgent,
    });

    await openReviewTab();
    await capturedOnCreateAgentWithReview!("review text", "acceptEdits", true);

    expect(onSendToIdleAgent).toHaveBeenCalledWith(84, "review text");
    expect(onSessionCreated).not.toHaveBeenCalled();
  });

  it("opens a new agent session for Edit by default, even when an agent is idle", async () => {
    const onSessionCreated = vi.fn();
    const onSendToIdleAgent = vi.fn();
    renderWorkspace(onSessionCreated, {
      idleAgentSession: IDLE_AGENT,
      onSendToIdleAgent,
    });

    await openReviewTab();
    await capturedOnCreateAgentWithReview!("review text", "acceptEdits");

    expect(onSendToIdleAgent).not.toHaveBeenCalled();
    expect(onSessionCreated).toHaveBeenCalledOnce();
  });

  it("Start Review opens a new agent session even when an agent is idle", async () => {
    const onSessionCreated = vi.fn();
    const onSendToIdleAgent = vi.fn();
    renderWorkspace(onSessionCreated, {
      idleAgentSession: IDLE_AGENT,
      onSendToIdleAgent,
    });

    await openReviewTab();
    await waitFor(() => expect(capturedOnStartAgentReview).toBeDefined());
    await capturedOnStartAgentReview!("review prompt", "codex");

    expect(onSendToIdleAgent).not.toHaveBeenCalled();
    expect(onSessionCreated).toHaveBeenCalledOnce();
    const [sessionInfo] = onSessionCreated.mock.calls[0] as [
      SessionCreationInfo,
    ];
    expect(sessionInfo.pendingPrompt).toBe("review prompt");
    expect(sessionInfo.permissionMode).toBe("acceptEdits");
    expect(sessionInfo.agent).toBe("codex");
  });
});
