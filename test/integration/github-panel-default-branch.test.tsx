// @include-parallel
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitHubPanel } from "../../src/components/GitHubPanel";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo } from "../utils";

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

vi.mock("../../src/hooks/useMergeQueueStatus", () => ({
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
vi.mock("../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../src/lib/api")>();
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
vi.mock("../../src/lib/api-pr-status", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/api-pr-status")>()),
  listCachedPrStatuses: api.listCachedPrStatuses,
}));

describe("GitHubPanel new pull request default branch", () => {
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

  it("prefills the new pull request base with the repo default branch", async () => {
    const { repoPath, defaultBranch } = createTestRepo(false);
    expect(defaultBranch).not.toBe("main");

    render(<GitHubPanel repoPath={repoPath} />);
    await user.click(screen.getByRole("button", { name: /new/i }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("tab", { name: /from branch/i }));

    const base = screen.getByPlaceholderText("Base branch");
    await waitFor(() => expect(base).toHaveValue(defaultBranch));

    await user.clear(base);
    await user.type(base, "release");
    expect(base).toHaveValue("release");
  });
});
