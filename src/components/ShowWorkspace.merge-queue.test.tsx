import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "../../test/test-utils";
import type { Workspace } from "../lib/api";
import * as api from "../lib/api";
import { ShowWorkspace } from "./ShowWorkspace";

const { mockFeatures } = vi.hoisted(() => ({
  mockFeatures: {
    pro: true,
    stripePayments: false,
    emailSignup: false,
    mergeQueue: false,
  },
}));

vi.mock("../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/features")>();
  return {
    ...actual,
    FEATURES: mockFeatures,
  };
});

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
    jjGetBranches: vi.fn().mockResolvedValue([
      { name: "main", is_current: false },
      { name: "feature-one", is_current: true },
    ]),
    setWorkspaceTargetBranch: vi.fn().mockResolvedValue(undefined),
    jjGetChangedFiles: vi.fn().mockResolvedValue([]),
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
      commits: [],
      edges: [],
      has_more: false,
    }),
    getWorkspaceStatus: vi.fn(),
  };
});

const { mockQueueEnabled } = vi.hoisted(() => ({
  mockQueueEnabled: { current: true },
}));

vi.mock("../hooks/useMergeQueueStatus", () => ({
  useMergeQueueStatus: () => ({ data: null }),
  useMergeQueueEnabled: () => ({ data: mockQueueEnabled.current }),
  useSetMergeQueueEnabled: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useEnqueueWorkspace: () => ({
    enqueue: { mutateAsync: vi.fn(), isPending: false },
    dequeue: { mutateAsync: vi.fn(), isPending: false },
  }),
  useGitRemoteInfo: () => ({ data: null }),
  usePrInfoViaGh: () => ({ data: null, isLoading: false }),
  usePrCiStatus: () => ({ data: null, isLoading: false }),
  createPrMutationKey: (
    repoPath: string | undefined,
    workspaceId: number | null | undefined,
  ) => ["create-pr", repoPath, workspaceId ?? null],
}));

describe("ShowWorkspace - Add to Queue feature flag", () => {
  const workspace: Workspace = {
    id: 1,
    repo_path: "/Users/test/repo",
    workspace_name: "feature-one",
    workspace_path: "/Users/test/repo/.jj/workspaces/feature-one",
    branch_name: "feature-one",
    title: "feature-one",
    created_at: new Date().toISOString(),
    not_on_remote: false,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockFeatures.mergeQueue = false;
    mockQueueEnabled.current = true;
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue({
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
    });
  });

  it("hides Add to Queue when mergeQueue feature flag is off", async () => {
    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
        onOpenMergePreview={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("button", { name: /merge/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /add to queue/i }),
    ).not.toBeInTheDocument();
  });

  it("shows Add to Queue when mergeQueue feature flag is on", async () => {
    mockFeatures.mergeQueue = true;

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
        onOpenMergePreview={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("button", { name: /add to queue/i }),
    ).toBeInTheDocument();
  });

  it("hides Add to Queue when the repo has not enabled the merge queue", async () => {
    mockFeatures.mergeQueue = true;
    mockQueueEnabled.current = false;

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
        onOpenMergePreview={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("button", { name: /merge/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /add to queue/i }),
    ).not.toBeInTheDocument();
  });
});
