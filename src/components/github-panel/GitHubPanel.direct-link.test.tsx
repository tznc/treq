import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../GitHubPanel";
import { render, screen } from "../../../test/test-utils";

const remoteInfo = vi.hoisted(() => ({
  data: null as { full_name: string; owner: string; name: string } | null,
  isLoading: true,
}));

const api = vi.hoisted(() => ({
  ghListIssues: vi.fn(),
  ghListPrs: vi.fn(),
  ghViewIssue: vi.fn(),
  ghViewPr: vi.fn(),
  getWorkspaces: vi.fn(),
}));

vi.mock("../../hooks/useMergeQueueStatus", () => ({
  useGitRemoteInfo: () => remoteInfo,
  usePrChecksForPr: () => ({ data: null, isLoading: false }),
  useMergeQueueEnabled: () => ({ data: false, isLoading: false }),
  useSetMergeQueueEnabled: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDequeueBranches: () => ({ mutate: vi.fn(), isPending: false }),
}));
vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return { ...original, ...api };
});

function makeItem(number: number, title: string) {
  return {
    number,
    title,
    state: "OPEN",
    url: `https://github.com/acme/treq/issues/${number}`,
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

function rejectEmptyRepo(item: ReturnType<typeof makeItem>) {
  return async (repo: string) => {
    if (!repo) throw new Error('gh: expected the "[HOST/]OWNER/REPO" format');
    return item;
  };
}

async function resolveRemote(rerender: (ui: React.ReactElement) => void) {
  remoteInfo.data = { full_name: "acme/treq", owner: "acme", name: "treq" };
  remoteInfo.isLoading = false;
  rerender(<GitHubPanel repoPath="/tmp/repo" />);
}

describe("GitHubPanel direct links while the remote loads", () => {
  beforeEach(() => {
    remoteInfo.data = null;
    remoteInfo.isLoading = true;
    for (const fn of Object.values(api)) fn.mockReset();
    api.ghListIssues.mockResolvedValue({ items: [], hasMore: false });
    api.ghListPrs.mockResolvedValue({ items: [], hasMore: false });
    api.getWorkspaces.mockResolvedValue([]);
  });

  it("waits for the remote before loading a linked issue", async () => {
    api.ghViewIssue.mockImplementation(
      rejectEmptyRepo(makeItem(42, "Linked issue")),
    );
    window.location.hash = "#/github/issues/open/42";

    const { rerender } = render(<GitHubPanel repoPath="/tmp/repo" />);
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(api.ghViewIssue).not.toHaveBeenCalled();
    expect(
      screen.queryByText("Could not load issue #42"),
    ).not.toBeInTheDocument();

    await resolveRemote(rerender);

    expect(
      await screen.findByRole("heading", { name: "Linked issue" }),
    ).toBeVisible();
    expect(api.ghViewIssue).toHaveBeenCalledTimes(1);
    expect(api.ghViewIssue).toHaveBeenCalledWith("acme/treq", 42);
  });

  it("waits for the remote before loading a linked pull request", async () => {
    api.ghViewPr.mockImplementation(rejectEmptyRepo(makeItem(7, "Linked PR")));
    window.location.hash = "#/github/prs/open/7";

    const { rerender } = render(<GitHubPanel repoPath="/tmp/repo" />);
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(api.ghViewPr).not.toHaveBeenCalled();
    expect(
      screen.queryByText("Could not load pull request #7"),
    ).not.toBeInTheDocument();

    await resolveRemote(rerender);

    expect(
      await screen.findByRole("heading", { name: "Linked PR" }),
    ).toBeVisible();
    expect(api.ghViewPr).toHaveBeenCalledWith("acme/treq", 7);
  });
});
