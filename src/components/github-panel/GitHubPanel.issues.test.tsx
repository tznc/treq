import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../GitHubPanel";
import { render, screen, waitFor, within } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghListIssues: vi.fn(),
  ghListPrs: vi.fn(),
  ghViewIssue: vi.fn(),
  ghDeleteIssue: vi.fn(),
  getWorkspaces: vi.fn(),
}));

vi.mock("../../hooks/useMergeQueueStatus", () => ({
  useGitRemoteInfo: () => ({
    data: { full_name: "acme/treq", owner: "acme", name: "treq" },
    isLoading: false,
  }),
  usePrChecksForPr: () => ({ data: null, isLoading: false }),
  useMergeQueueEnabled: () => ({ data: false, isLoading: false }),
  useSetMergeQueueEnabled: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDequeueBranches: () => ({ mutate: vi.fn(), isPending: false }),
}));
vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return { ...original, ...api };
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

describe("GitHubPanel delete issue", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let deleted: boolean;

  beforeEach(() => {
    window.location.hash = "";
    user = userEvent.setup();
    deleted = false;
    for (const fn of Object.values(api)) fn.mockReset();
    api.ghListIssues.mockImplementation(async () => ({
      items: deleted
        ? [makeIssue(7, "Keep me")]
        : [makeIssue(42, "Delete me"), makeIssue(7, "Keep me")],
      hasMore: false,
    }));
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.ghViewIssue.mockResolvedValue(makeIssue(42, "Delete me"));
    api.ghDeleteIssue.mockImplementation(async () => {
      deleted = true;
    });
    api.getWorkspaces.mockResolvedValue([]);
  });

  async function openIssueAndAskToDelete() {
    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(await screen.findByRole("button", { name: /delete me/i }));
    await user.click(
      await screen.findByRole("button", { name: /delete issue/i }),
    );
    return screen.findByRole("dialog");
  }

  it("deletes the issue after confirmation and returns to the list", async () => {
    const dialog = await openIssueAndAskToDelete();
    expect(within(dialog).getByText(/delete issue #42\?/i)).toBeVisible();

    await user.click(within(dialog).getByRole("button", { name: /^delete$/i }));

    await waitFor(() =>
      expect(api.ghDeleteIssue).toHaveBeenCalledWith("acme/treq", 42),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Delete me" }),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /delete me/i }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: /keep me/i })).toBeVisible();
  });

  it("does nothing when the confirmation is cancelled", async () => {
    const dialog = await openIssueAndAskToDelete();

    await user.click(within(dialog).getByRole("button", { name: /cancel/i }));

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(api.ghDeleteIssue).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Delete me" })).toBeVisible();
  });

  it("keeps the issue open and shows the gh error when deleting fails", async () => {
    api.ghDeleteIssue.mockRejectedValue(
      "gh: HTTP 403: Must have admin rights to Repository.",
    );
    const dialog = await openIssueAndAskToDelete();

    await user.click(within(dialog).getByRole("button", { name: /^delete$/i }));

    expect(await screen.findByText("Failed to delete issue")).toBeVisible();
    expect(
      screen.getByText("gh: HTTP 403: Must have admin rights to Repository."),
    ).toBeVisible();
    expect(screen.getByRole("heading", { name: "Delete me" })).toBeVisible();
  });
});

describe("GitHubPanel issue pagination", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    window.location.hash = "";
    user = userEvent.setup();
    for (const fn of Object.values(api)) fn.mockReset();
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.getWorkspaces.mockResolvedValue([]);
  });

  it("requests each page after the previous page's cursor", async () => {
    api.ghListIssues.mockImplementation(async (...args: unknown[]) =>
      args[3] === "cursor-2"
        ? { items: [makeIssue(1)], hasMore: false, endCursor: "cursor-3" }
        : args[3] === "cursor-1"
          ? {
              items: [makeIssue(3), makeIssue(2)],
              hasMore: true,
              endCursor: "cursor-2",
            }
          : {
              items: [makeIssue(5), makeIssue(4)],
              hasMore: true,
              endCursor: "cursor-1",
            },
    );

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Issue 5");
    await user.click(screen.getByRole("button", { name: /load more/i }));
    await screen.findByText("Issue 3");
    await user.click(screen.getByRole("button", { name: /load more/i }));

    expect(await screen.findByText("Issue 1")).toBeVisible();
    for (const n of [2, 3, 4, 5]) {
      expect(screen.getByText(`Issue ${n}`)).toBeVisible();
    }
    expect(
      screen.queryByRole("button", { name: /load more/i }),
    ).not.toBeInTheDocument();
  });

  it("lists an issue once when two pages both return it", async () => {
    api.ghListIssues.mockImplementation(async (...args: unknown[]) =>
      args[3] === "cursor-1"
        ? {
            items: [makeIssue(2), makeIssue(1)],
            hasMore: false,
            endCursor: "cursor-2",
          }
        : {
            items: [makeIssue(3), makeIssue(2)],
            hasMore: true,
            endCursor: "cursor-1",
          },
    );

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await screen.findByText("Issue 3");
    await user.click(screen.getByRole("button", { name: /load more/i }));

    expect(await screen.findByText("Issue 1")).toBeVisible();
    await waitFor(() =>
      expect(screen.getAllByRole("button", { name: /issue 2/i })).toHaveLength(
        1,
      ),
    );
    const titles = screen
      .getAllByRole("button", { name: /^issue \d/i })
      .map((b) => b.textContent);
    expect(titles).toEqual([
      expect.stringContaining("Issue 3"),
      expect.stringContaining("Issue 2"),
      expect.stringContaining("Issue 1"),
    ]);
  });
});
