import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../GitHubPanel";
import { render, screen, waitFor, within } from "../../../test/test-utils";

const remoteInfo = vi.hoisted(() => ({
  data: { full_name: "acme/treq", owner: "acme", name: "treq" },
  isLoading: false,
}));

const api = vi.hoisted(() => ({
  ghListPrs: vi.fn(),
  getWorkspaces: vi.fn(),
  ghCreatePr: vi.fn(),
  pushWorkspaceToRemote: vi.fn(),
  listCachedPrStatuses: vi.fn(),
  getRepoDefaultBranch: vi.fn(),
}));

vi.mock("../../hooks/useMergeQueueStatus", () => ({
  cachedPrStatusesKey: (repoPath: string) => ["cached-pr-statuses", repoPath],
  createPrMutationKey: (repoPath: string, id: number | null) => [
    "create-pr",
    repoPath,
    id,
  ],
  invalidatePrStatuses: vi.fn(),
  useGitRemoteInfo: () => remoteInfo,
  usePrChecksForPr: () => ({ data: null, isLoading: false }),
  useMergeQueueEnabled: () => ({ data: false, isLoading: false }),
  useDequeueBranches: () => ({ mutate: vi.fn(), isPending: false }),
}));
vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  api.getRepoDefaultBranch.mockImplementation(actual.getRepoDefaultBranch);
  return {
    ...actual,
    ghListPrs: api.ghListPrs,
    getWorkspaces: api.getWorkspaces,
    ghCreatePr: api.ghCreatePr,
    pushWorkspaceToRemote: api.pushWorkspaceToRemote,
    getRepoDefaultBranch: api.getRepoDefaultBranch,
  };
});
vi.mock("../../lib/api-pr-status", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api-pr-status")>()),
  listCachedPrStatuses: api.listCachedPrStatuses,
}));

function makePr(number: number, title = `PR ${number}`) {
  return {
    number,
    title,
    state: "OPEN",
    url: `https://github.com/acme/treq/pull/${number}`,
    body: null,
    author: { login: "alice" },
    labels: [] as { name: string; color: string }[],
    head_ref_name: `feat/${number}`,
    base_ref_name: "main",
    merge_state_status: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    comments: null,
  };
}

describe("GitHubPanel pull request list", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    window.location.hash = "#/github/prs/open";
    api.ghListPrs.mockReset();
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.getWorkspaces.mockResolvedValue([]);
    api.ghCreatePr.mockReset();
    api.pushWorkspaceToRemote.mockReset();
    api.pushWorkspaceToRemote.mockResolvedValue(undefined);
    api.listCachedPrStatuses.mockResolvedValue({});
    user = userEvent.setup();
  });

  it("shows who opened each pull request on its second row", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [{ ...makePr(1), author: { login: "octocat" } }],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);

    const row = (await screen.findByText("PR 1")).closest("button")!;
    expect(within(row).getByText("octocat")).toBeVisible();
  });

  it("narrows the pull request list with the search bar", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [makePr(1, "Fix login"), makePr(2, "Add billing")],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Fix login");

    await user.type(
      screen.getByRole("searchbox", { name: /search pull requests/i }),
      "billing",
    );

    expect(screen.getByText("Add billing")).toBeVisible();
    expect(screen.queryByText("Fix login")).not.toBeInTheDocument();
  });

  it("puts the Filters button and search input on the state filters row", async () => {
    render(<GitHubPanel repoPath="/tmp/repo" />);

    const all = await screen.findByRole("button", { name: "All" });
    const row = all.parentElement!.parentElement!;
    const filters = within(row).getByRole("button", { name: /^filters/i });
    const search = within(row).getByRole("searchbox", {
      name: /search pull requests/i,
    });
    expect(all.compareDocumentPosition(filters)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(filters.compareDocumentPosition(search)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("ANDs the author and label filters", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [
        {
          ...makePr(1, "Alice bug"),
          labels: [{ name: "bug", color: "ff0000" }],
        },
        { ...makePr(2, "Alice feature") },
        {
          ...makePr(3, "Bob bug"),
          author: { login: "bob" },
          labels: [{ name: "bug", color: "ff0000" }],
        },
      ],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Alice bug");

    await user.click(screen.getByRole("button", { name: /^filters/i }));
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Author" }),
      "alice",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Label" }),
      "bug",
    );

    expect(screen.getByText("Alice bug")).toBeVisible();
    expect(screen.queryByText("Alice feature")).not.toBeInTheDocument();
    expect(screen.queryByText("Bob bug")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^filters/i }));
    expect(
      screen.queryByRole("combobox", { name: "Author" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Bob bug")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^filters/i }));
    await user.click(
      await screen.findByRole("button", { name: /clear filters/i }),
    );
    expect(screen.getByText("Bob bug")).toBeVisible();
  });

  it("keeps showing a selected filter value no loaded pull request has", async () => {
    api.ghListPrs.mockImplementation(async (_repo: string, state: string) => ({
      items:
        state === "closed"
          ? [makePr(2, "Alice closed")]
          : [{ ...makePr(1, "Bob open"), author: { login: "bob" } }],
      hasMore: false,
    }));

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Bob open");
    await user.click(screen.getByRole("button", { name: /^filters/i }));
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Author" }),
      "bob",
    );
    await user.click(screen.getByRole("button", { name: /^filters/i }));

    await user.click(screen.getByRole("button", { name: "Closed" }));
    expect(
      await screen.findByText("No loaded pull requests match the filters."),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: /^filters/i }));
    expect(screen.getByRole("combobox", { name: "Author" })).toHaveValue("bob");
  });

  it("filters pull requests by stack level", async () => {
    api.ghListPrs.mockResolvedValue({
      items: [
        { ...makePr(1, "Bottom"), head_ref_name: "a" },
        { ...makePr(2, "Top"), head_ref_name: "b", base_ref_name: "a" },
      ],
      hasMore: false,
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Bottom");

    await user.click(screen.getByRole("button", { name: /^filters/i }));
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Stack level" }),
      "2",
    );

    expect(screen.getByText("Top")).toBeVisible();
    expect(screen.queryByText("Bottom")).not.toBeInTheDocument();
  });

  it("opens new pull requests in a modal with workspace and branch tabs", async () => {
    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("button", { name: /new/i }));

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("tab", { name: /from workspace/i }),
    ).toHaveAttribute("aria-selected", "true");
    await user.click(within(dialog).getByRole("tab", { name: /from branch/i }));
    expect(within(dialog).getByPlaceholderText("Head branch")).toBeVisible();

    await user.click(within(dialog).getByRole("button", { name: /cancel/i }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("offers only workspaces without an open pull request and creates one", async () => {
    const workspace = (id: number, branch: string, title: string) => ({
      id,
      repo_path: "/tmp/repo",
      workspace_name: branch,
      workspace_path: `/tmp/ws/${id}`,
      branch_name: branch,
      created_at: "2026-01-01T00:00:00Z",
      target_branch: "main",
      title,
      description: "Body text",
      not_on_remote: true,
    });
    api.getWorkspaces.mockResolvedValue([
      workspace(1, "feat/has-pr", "Has PR"),
      workspace(2, "feat/closed-pr", "Closed PR"),
      workspace(3, "feat/no-pr", "No PR yet"),
    ]);
    api.listCachedPrStatuses.mockResolvedValue({
      "feat/has-pr": { number: 5, state: "OPEN" },
      "feat/closed-pr": { number: 6, state: "CLOSED" },
    });
    api.ghCreatePr.mockResolvedValue(42);

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("button", { name: /new/i }));
    const dialog = await screen.findByRole("dialog");

    await user.click(
      await within(dialog).findByRole("radio", { name: /no pr yet/i }),
    );
    expect(
      within(dialog).getByRole("radio", { name: /closed pr/i }),
    ).toBeVisible();
    expect(
      within(dialog).queryByRole("radio", { name: /has pr/i }),
    ).not.toBeInTheDocument();

    await user.click(
      within(dialog).getByRole("button", { name: /create pull request/i }),
    );

    await waitFor(() =>
      expect(api.ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        expect.any(String),
        "Body text",
        "main",
        "feat/no-pr",
        false,
      ),
    );
    expect(api.pushWorkspaceToRemote).toHaveBeenCalledWith("/tmp/repo", 3);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("lists workspaces only once the repo default branch is known", async () => {
    let resolveDefaultBranch!: (branch: string) => void;
    api.getRepoDefaultBranch.mockImplementationOnce(
      () => new Promise<string>((resolve) => (resolveDefaultBranch = resolve)),
    );
    const workspace = (id: number, branch: string, title: string) => ({
      id,
      repo_path: "/tmp/repo",
      workspace_name: branch,
      workspace_path: `/tmp/ws/${id}`,
      branch_name: branch,
      created_at: "2026-01-01T00:00:00Z",
      target_branch: null,
      title,
    });
    api.getWorkspaces.mockResolvedValue([
      workspace(1, "trunk", "Home"),
      workspace(2, "feat/no-pr", "No PR yet"),
    ]);

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("button", { name: /new/i }));
    const dialog = await screen.findByRole("dialog");

    await expect(
      within(dialog).findByRole("radio", { name: /home/i }),
    ).rejects.toThrow();
    expect(within(dialog).getByText("Loading workspaces…")).toBeVisible();

    resolveDefaultBranch("trunk");
    await user.click(
      await within(dialog).findByRole("radio", { name: /no pr yet/i }),
    );
    expect(
      within(dialog).queryByRole("radio", { name: /home/i }),
    ).not.toBeInTheDocument();
    expect(within(dialog).getByLabelText("Base branch")).toHaveValue("trunk");
  });

  it("lists workspaces when the repo default branch cannot be read", async () => {
    api.getRepoDefaultBranch.mockImplementationOnce(() =>
      Promise.reject(new Error("no default branch")),
    );
    api.getWorkspaces.mockResolvedValue([
      {
        id: 2,
        repo_path: "/tmp/repo",
        workspace_name: "feat/no-pr",
        workspace_path: "/tmp/ws/2",
        branch_name: "feat/no-pr",
        created_at: "2026-01-01T00:00:00Z",
        target_branch: "main",
        title: "No PR yet",
      },
    ]);

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("button", { name: /new/i }));
    const dialog = await screen.findByRole("dialog");

    await user.click(
      await within(dialog).findByRole("radio", { name: /no pr yet/i }),
    );
    expect(within(dialog).getByLabelText("Base branch")).toHaveValue("main");
  });
});
