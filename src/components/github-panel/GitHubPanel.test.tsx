import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../GitHubPanel";
import { render, screen, within } from "../../../test/test-utils";

const auth = vi.hoisted(() => ({
  user: { id: "user-1" } as object | null,
  session: { access_token: "token" } as object | null,
  loading: false,
  subscription: null as { plan: string; status: string } | null,
  signIn: vi.fn(),
}));

const remoteInfo = vi.hoisted(() => ({
  data: { full_name: "acme/treq", owner: "acme", name: "treq" } as {
    full_name: string;
    owner: string;
    name: string;
  } | null,
  isLoading: false,
}));

const api = vi.hoisted(() => ({
  ghListIssues: vi.fn(),
  ghListPrs: vi.fn(),
  ghCreateIssueComment: vi.fn(),
  getWorkspaces: vi.fn(),
  openOrCreateWorkspaceFromPr: vi.fn(),
}));

const supabaseRpc = vi.hoisted(() => vi.fn());

const queueEnabled = vi.hoisted(() => ({
  current: true,
  setEnabled: vi.fn(),
  dequeue: vi.fn(),
}));

const mockFeatures = vi.hoisted(() => ({ mergeQueue: true }));

vi.mock("../../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/features")>();
  return {
    ...actual,
    FEATURES: Object.assign(mockFeatures, {
      ...actual.FEATURES,
      ...mockFeatures,
    }),
  };
});

vi.mock("../../stores/authStore", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../stores/authStore")>();
  const { createAuthStoreMock } = await import(
    "../../../test/mocks/zustandAuthStore"
  );
  return { ...actual, useAuthStore: createAuthStoreMock(auth) };
});
vi.mock("../../hooks/useMergeQueueStatus", () => ({
  useGitRemoteInfo: () => remoteInfo,
  usePrChecksForPr: () => ({ data: null, isLoading: false }),
  useMergeQueueEnabled: () => ({
    data: queueEnabled.current,
    isLoading: false,
  }),
  useSetMergeQueueEnabled: () => ({
    mutateAsync: queueEnabled.setEnabled,
    isPending: false,
  }),
  useDequeueBranches: () => ({
    mutate: queueEnabled.dequeue,
    isPending: false,
  }),
}));
vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...original,
    ghListIssues: api.ghListIssues,
    ghListPrs: api.ghListPrs,
    ghCreateIssueComment: api.ghCreateIssueComment,
    getWorkspaces: api.getWorkspaces,
    openOrCreateWorkspaceFromPr: api.openOrCreateWorkspaceFromPr,
  };
});
vi.mock("../../lib/supabase", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/supabase")>();
  return {
    ...original,
    supabase: { rpc: supabaseRpc },
    WEB_URL: "http://localhost:3001",
  };
});

function makeIssue(number: number, title = `Issue ${number}`) {
  return {
    number,
    title,
    state: "OPEN",
    url: `https://github.com/acme/treq/issues/${number}`,
    body: null,
    author: { login: "alice" },
    labels: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    comments: null,
  };
}

function makePr(number: number, title = `PR ${number}`) {
  return {
    number,
    title,
    state: "OPEN",
    url: `https://github.com/acme/treq/pull/${number}`,
    body: null,
    author: { login: "alice" },
    labels: [],
    head_ref_name: `feat/${number}`,
    base_ref_name: "main",
    merge_state_status: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    comments: null,
  };
}

describe("GitHubPanel", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    window.location.hash = "";
    auth.subscription = null;
    mockFeatures.mergeQueue = true;
    queueEnabled.current = true;
    queueEnabled.setEnabled.mockReset();
    queueEnabled.dequeue.mockReset();
    remoteInfo.data = {
      full_name: "acme/treq",
      owner: "acme",
      name: "treq",
    };
    remoteInfo.isLoading = false;
    api.ghListIssues.mockResolvedValue({ items: [], hasMore: false });
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.ghCreateIssueComment.mockReset();
    api.getWorkspaces.mockResolvedValue([]);
    api.openOrCreateWorkspaceFromPr.mockReset();
    supabaseRpc.mockReset();
    supabaseRpc.mockResolvedValue({ data: [], error: null });
    user = userEvent.setup();
  });

  it("does not render a panel-level close button", () => {
    render(<GitHubPanel repoPath="/tmp/repo" />);
    const header = screen
      .getByRole("heading", { name: /github/i })
      .closest("div")?.parentElement;
    expect(header).toBeTruthy();
    expect(within(header!).queryByRole("button")).not.toBeInTheDocument();
  });

  it("hides the Merge Queue tab and ignores a direct link when the flag is off", async () => {
    mockFeatures.mergeQueue = false;
    auth.subscription = { plan: "pro", status: "active" };
    window.location.hash = "#/github/merge-queue";

    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(
      screen.queryByRole("tab", { name: /merge queue/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Issues" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByText("No issues found.")).toBeVisible();
    expect(screen.queryByTestId("merge-queue-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("merge-queue-toolbar")).not.toBeInTheDocument();
    expect(supabaseRpc).not.toHaveBeenCalled();
  });

  it("lets Free users open Merge Queue and shows an upgrade upsell", async () => {
    auth.subscription = { plan: "free", status: "active" };
    render(<GitHubPanel repoPath="/tmp/repo" />);

    const mergeQueueTab = screen.getByRole("tab", { name: /merge queue/i });
    expect(mergeQueueTab).not.toBeDisabled();
    expect(within(mergeQueueTab).getByText("PRO")).toBeVisible();

    await user.click(mergeQueueTab);
    expect(mergeQueueTab).toHaveAttribute("aria-selected", "true");
    expect(
      await screen.findByRole("heading", { name: /unlock merge queue/i }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: /upgrade to pro/i }),
    ).toBeVisible();
    expect(supabaseRpc).not.toHaveBeenCalled();
  });

  it("enables Merge Queue tab for active Pro users and shows queue entries", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    supabaseRpc.mockResolvedValue({
      data: [
        {
          branch_name: "feat/alpha",
          pr_number: 11,
          status: "queued",
          position: 1,
          target_branch: "main",
        },
        {
          branch_name: "feat/beta",
          pr_number: 12,
          status: "testing",
          position: 2,
          target_branch: "main",
        },
      ],
      error: null,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    const mergeQueueTab = screen.getByRole("tab", { name: /merge queue/i });
    expect(mergeQueueTab).not.toBeDisabled();
    expect(within(mergeQueueTab).queryByText("PRO")).not.toBeInTheDocument();

    await user.click(mergeQueueTab);
    expect(await screen.findByText("PR #11")).toBeVisible();
    expect(screen.getByText("PR #12")).toBeVisible();
    expect(screen.getByText("feat/alpha → main")).toBeVisible();
    expect(screen.getByText("feat/beta → main")).toBeVisible();
    expect(screen.getByText(/queued/i)).toBeVisible();
    expect(screen.getByText(/running checks/i)).toBeVisible();
  });

  it("hides the queue and points at Settings when the repo has it disabled", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    queueEnabled.current = false;

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /merge queue/i }));

    expect(
      await screen.findByText(/merge queue is off for this repository/i),
    ).toBeVisible();
    expect(
      screen.queryByRole("switch", { name: /enable merge queue/i }),
    ).not.toBeInTheDocument();
    expect(supabaseRpc).not.toHaveBeenCalled();
  });

  it("opens the integrations settings tab from the disabled queue state", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    queueEnabled.current = false;
    const onOpenSettings = vi.fn();

    render(
      <GitHubPanel repoPath="/tmp/repo" onOpenSettings={onOpenSettings} />,
    );
    await user.click(screen.getByRole("tab", { name: /merge queue/i }));
    await user.click(
      await screen.findByRole("button", { name: /enable it in settings/i }),
    );

    expect(onOpenSettings).toHaveBeenCalledWith("integrations");
  });

  it("groups stacked branches into a block and removes them together", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    supabaseRpc.mockResolvedValue({
      data: [
        {
          branch_name: "feat/base",
          pr_number: 11,
          status: "queued",
          position: 1,
          target_branch: "main",
        },
        {
          branch_name: "feat/top",
          pr_number: 12,
          status: "queued",
          position: 2,
          target_branch: "feat/base",
        },
      ],
      error: null,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /merge queue/i }));

    expect(await screen.findByText("Stack of 2")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "What is a stack?" }),
    ).toBeVisible();
    expect(
      screen.queryByText(/merges bottom-up into main/i),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Remove stack of 2 from queue" }),
    );
    expect(queueEnabled.dequeue).toHaveBeenCalledWith([
      "feat/base",
      "feat/top",
    ]);
    expect(
      screen.queryByRole("button", { name: "Remove feat/base from queue" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Remove feat/top from queue" }),
    ).not.toBeInTheDocument();
  });

  it("removes only the branch itself when it has nothing stacked on it", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    supabaseRpc.mockResolvedValue({
      data: [
        {
          branch_name: "fix/solo",
          pr_number: 11,
          status: "queued",
          position: 1,
          target_branch: "main",
        },
      ],
      error: null,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /merge queue/i }));

    await screen.findByText("PR #11");
    expect(screen.queryByText(/^Stack of/)).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Remove fix/solo from queue" }),
    );
    expect(queueEnabled.dequeue).toHaveBeenCalledWith(["fix/solo"]);
  });

  it("shows Load more when more issues are available", async () => {
    api.ghListIssues.mockResolvedValue({
      items: [makeIssue(1), makeIssue(2)],
      hasMore: true,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(await screen.findByText("Issue 1")).toBeVisible();
    expect(screen.getByRole("button", { name: /load more/i })).toBeVisible();
  });

  it("does not show Load more when all issues are loaded", async () => {
    api.ghListIssues.mockResolvedValue({
      items: [makeIssue(1)],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(await screen.findByText("Issue 1")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /load more/i }),
    ).not.toBeInTheDocument();
  });

  it("loads the next page of issues when Load more is clicked", async () => {
    api.ghListIssues
      .mockResolvedValueOnce({
        items: [makeIssue(1, "First page issue")],
        hasMore: true,
        endCursor: "cursor-1",
      })
      .mockResolvedValueOnce({
        items: [makeIssue(2, "Second page issue")],
        hasMore: false,
      });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(await screen.findByText("First page issue")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /load more/i }));

    expect(await screen.findByText("Second page issue")).toBeVisible();
    expect(screen.getByText("First page issue")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /load more/i }),
    ).not.toBeInTheDocument();
    expect(api.ghListIssues).toHaveBeenCalledWith(
      "acme/treq",
      "open",
      expect.any(Number),
      "cursor-1",
    );
  });

  it("shows Load more when more pull requests are available", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [makePr(1), makePr(2)],
      hasMore: true,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    expect(await screen.findByText("PR 1")).toBeVisible();
    expect(screen.getByRole("button", { name: /load more/i })).toBeVisible();
  });

  it("places a Draft filter to the left of Open on the pull request list", async () => {
    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    const draftFilter = await screen.findByRole("button", { name: "Draft" });
    const openFilter = screen.getByRole("button", { name: "Open" });
    expect(draftFilter.compareDocumentPosition(openFilter)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("does not show a Draft filter on the issues list", async () => {
    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(await screen.findByRole("button", { name: "Open" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Draft" }),
    ).not.toBeInTheDocument();
  });

  it("lists only draft pull requests when the Draft filter is selected", async () => {
    api.ghListPrs.mockImplementation(async (_repo: string, state: string) => {
      if (state === "draft") {
        return {
          items: [{ ...makePr(2, "WIP PR"), is_draft: true }],
          hasMore: false,
        };
      }
      return {
        items: [
          { ...makePr(1, "Ready PR"), is_draft: false },
          { ...makePr(2, "WIP PR"), is_draft: true },
        ],
        hasMore: false,
      };
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    expect(await screen.findByText("Ready PR")).toBeVisible();
    expect(screen.getByText("WIP PR")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Draft" }));

    expect(await screen.findByText("WIP PR")).toBeVisible();
    expect(screen.queryByText("Ready PR")).not.toBeInTheDocument();
    expect(api.ghListPrs).toHaveBeenCalledWith(
      "acme/treq",
      "draft",
      expect.any(Number),
      1,
    );
  });

  it("marks draft pull requests as Draft instead of Open", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [
        { ...makePr(1, "Ready PR"), is_draft: false },
        { ...makePr(2, "WIP PR"), is_draft: true },
      ],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    const readyRow = (await screen.findByText("Ready PR")).closest("button")!;
    const draftRow = screen.getByText("WIP PR").closest("button")!;
    expect(within(readyRow).getByText("Open")).toBeVisible();
    expect(within(draftRow).getByText("Draft")).toBeVisible();
    expect(within(draftRow).queryByText("Open")).not.toBeInTheDocument();
  });

  it("loads the next page of pull requests when Load more is clicked", async () => {
    api.ghListPrs
      .mockResolvedValueOnce({
        items: [makePr(1, "First page PR")],
        hasMore: true,
      })
      .mockResolvedValueOnce({
        items: [makePr(2, "Second page PR")],
        hasMore: false,
      });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    expect(await screen.findByText("First page PR")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /load more/i }));

    expect(await screen.findByText("Second page PR")).toBeVisible();
    expect(screen.getByText("First page PR")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /load more/i }),
    ).not.toBeInTheDocument();
    expect(api.ghListPrs).toHaveBeenCalledWith(
      "acme/treq",
      "open",
      expect.any(Number),
      2,
    );
  });

  it("shows the gh error instead of an empty list when PRs fail to load", async () => {
    api.ghListPrs.mockRejectedValue(
      "gh: You are not logged into any GitHub hosts. Run gh auth login.",
    );

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));

    expect(
      await screen.findByText("Could not load pull requests"),
    ).toBeVisible();
    expect(
      screen.getByText(
        "gh: You are not logged into any GitHub hosts. Run gh auth login.",
      ),
    ).toBeVisible();
    expect(
      screen.queryByText("No pull requests found."),
    ).not.toBeInTheDocument();
  });

  it("retries a failed issue list load", async () => {
    api.ghListIssues
      .mockRejectedValueOnce(new Error("gh: HTTP 502"))
      .mockResolvedValue({
        items: [makeIssue(1, "Back again")],
        hasMore: false,
      });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    expect(await screen.findByText("Could not load issues")).toBeVisible();
    expect(screen.getByText("gh: HTTP 502")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /try again/i }));

    expect(await screen.findByText("Back again")).toBeVisible();
    expect(screen.queryByText("Could not load issues")).not.toBeInTheDocument();
  });
});
