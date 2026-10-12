import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PrDetailPanel } from "./PrDetail";
import { render, screen, waitFor } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghViewPr: vi.fn(),
  getWorkspaces: vi.fn(),
  openOrCreateWorkspaceFromPr: vi.fn(),
  ghSetPrDraft: vi.fn(),
  ghClosePr: vi.fn(),
  ghReopenPr: vi.fn(),
  ghCreatePrComment: vi.fn(),
}));

vi.mock("../../hooks/useMergeQueueStatus", () => ({
  usePrChecksForPr: () => ({ data: null, isLoading: false }),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...original,
    ghViewPr: api.ghViewPr,
    getWorkspaces: api.getWorkspaces,
    openOrCreateWorkspaceFromPr: api.openOrCreateWorkspaceFromPr,
    ghSetPrDraft: api.ghSetPrDraft,
    ghClosePr: api.ghClosePr,
    ghReopenPr: api.ghReopenPr,
    ghCreatePrComment: api.ghCreatePrComment,
  };
});

describe("PrDetailPanel create workspace from PR", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.getWorkspaces.mockReset().mockResolvedValue([]);
    api.openOrCreateWorkspaceFromPr.mockReset();
    api.ghViewPr.mockResolvedValue({
      number: 42,
      title: "Feature PR",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/42",
      body: "Body",
      author: { login: "alice" },
      labels: [],
      head_ref_name: "feat/thing",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      comments: null,
      is_draft: false,
    });
  });

  it("creates a workspace from the PR head and base branches", async () => {
    api.openOrCreateWorkspaceFromPr.mockResolvedValue({
      workspaceId: 7,
      created: true,
    });
    const onOpenWorkspace = vi.fn();

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
        onOpenWorkspace={onOpenWorkspace}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: /create workspace/i }),
    );

    await waitFor(() => {
      expect(api.openOrCreateWorkspaceFromPr).toHaveBeenCalledWith(
        "/tmp/repo",
        "feat/thing",
        "main",
        "Feature PR",
        "From GitHub PR #42",
      );
    });
    await waitFor(() => {
      expect(onOpenWorkspace).toHaveBeenCalledWith(7);
    });
  });

  it("shows Open Workspace when a matching workspace already exists", async () => {
    api.getWorkspaces.mockResolvedValue([
      {
        id: 3,
        repo_path: "/tmp/repo",
        workspace_name: "feat-thing",
        workspace_path: "feat-thing",
        branch_name: "feat/thing",
        created_at: "2026-01-01T00:00:00Z",
        title: "Feature PR",
        not_on_remote: false,
      },
    ]);
    api.openOrCreateWorkspaceFromPr.mockResolvedValue({
      workspaceId: 3,
      created: false,
    });
    const onOpenWorkspace = vi.fn();

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
        onOpenWorkspace={onOpenWorkspace}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: /open workspace/i }),
    );

    await waitFor(() => {
      expect(onOpenWorkspace).toHaveBeenCalledWith(3);
    });
  });
});
