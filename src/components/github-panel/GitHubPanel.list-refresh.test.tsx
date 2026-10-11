import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../GitHubPanel";
import { render, screen, waitFor } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghListIssues: vi.fn(),
  ghListPrs: vi.fn(),
  ghViewIssue: vi.fn(),
  ghCloseIssue: vi.fn(),
  ghViewPr: vi.fn(),
  ghClosePr: vi.fn(),
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

function makeItem(number: number, title: string, state = "OPEN") {
  return {
    number,
    title,
    state,
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

describe("GitHubPanel list refresh", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    window.location.hash = "";
    for (const fn of Object.values(api)) fn.mockReset();
    api.ghListIssues.mockResolvedValue({ items: [], hasMore: false });
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.getWorkspaces.mockResolvedValue([]);
    user = userEvent.setup();
  });

  it("drops a closed pull request from the Open list", async () => {
    let closed = false;
    api.ghListPrs.mockImplementation(async () => ({
      items: closed
        ? [makeItem(7, "Keep me open")]
        : [makeItem(42, "Close me"), makeItem(7, "Keep me open")],
      hasMore: false,
    }));
    api.ghViewPr.mockImplementation(async () =>
      makeItem(42, "Close me", closed ? "CLOSED" : "OPEN"),
    );
    api.ghClosePr.mockImplementation(async () => {
      closed = true;
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(screen.getByRole("tab", { name: /pull requests/i }));
    await user.click(await screen.findByRole("button", { name: /close me/i }));
    await user.click(await screen.findByRole("button", { name: /close pr/i }));

    expect(
      await screen.findByRole("button", { name: /reopen pr/i }),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /close me/i }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: /keep me open/i })).toBeVisible();
  });

  it("drops a closed issue from the Open list", async () => {
    let closed = false;
    api.ghListIssues.mockImplementation(async () => ({
      items: closed
        ? [makeItem(7, "Keep me open")]
        : [makeItem(42, "Close me"), makeItem(7, "Keep me open")],
      hasMore: false,
    }));
    api.ghViewIssue.mockImplementation(async () =>
      makeItem(42, "Close me", closed ? "CLOSED" : "OPEN"),
    );
    api.ghCloseIssue.mockImplementation(async () => {
      closed = true;
    });

    render(<GitHubPanel repoPath="/tmp/repo" />);
    await user.click(await screen.findByRole("button", { name: /close me/i }));
    await user.click(
      await screen.findByRole("button", { name: /close issue/i }),
    );

    expect(
      await screen.findByRole("button", { name: /reopen issue/i }),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /close me/i }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: /keep me open/i })).toBeVisible();
  });
});
