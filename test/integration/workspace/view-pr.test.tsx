// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getCachedPrInfo,
  getWorkspaces,
  ghViewPr,
  getPrInfoViaGh,
  pushWorkspaceToRemote,
  updateWorkspace,
} from "../../../src/lib/api";
import { render, screen, within } from "../../test-utils";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
  setOriginUrl,
} from "../../utils";

vi.mock("../../../src/lib/api", async (importOriginal) => {
  const original =
    await importOriginal<typeof import("../../../src/lib/api")>();
  return {
    ...original,
    getCachedPrInfo: vi.fn().mockResolvedValue(null),
    getPrInfoViaGh: vi.fn().mockResolvedValue(null),
    startPrStatusPolling: vi.fn(async () => undefined),
    stopPrStatusPolling: vi.fn(async () => undefined),
    refreshPrStatuses: vi.fn(async () => undefined),
    getPrChecksForPr: vi.fn().mockResolvedValue(null),
    ghViewPr: vi.fn(),
    ghListPrs: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
    ghListIssues: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
    pushWorkspaceToRemote: vi.fn(
      (repoPath: string, workspaceId: number | null) =>
        original.pushWorkspaceToRemote(repoPath, workspaceId),
    ),
  };
});

describe("ShowWorkspace - View PR", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(true));
    openRepo(repoPath);
    user = userEvent.setup();
    vi.mocked(getCachedPrInfo).mockReset().mockResolvedValue(null);
    vi.mocked(getPrInfoViaGh).mockReset().mockResolvedValue(null);
    vi.mocked(openUrl).mockReset();
  });

  async function setupPushedWorkspaceWithGitHub() {
    const workspaceId = await createWorkspace(repoPath, "feat/create-pr");
    await updateWorkspace(
      repoPath,
      workspaceId,
      undefined,
      "Ship the feature",
      "Implements the feature end-to-end.",
    );
    const created = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    )!;
    await commitWorkspaceFile(
      repoPath,
      { id: created.id, path: created.workspace_path },
      "feature.txt",
      "feature content",
      "Add feature",
    );
    await pushWorkspaceToRemote(repoPath, workspaceId);
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
  }

  async function openWorkspace(branchName: string) {
    await clickGpuiRow(branchName);
    return screen.findByTestId("show-workspace-header");
  }

  it("navigates to in-app GitHub PR detail on View PR; Open on Web opens browser", async () => {
    await setupPushedWorkspaceWithGitHub();
    vi.mocked(getCachedPrInfo).mockResolvedValue({
      number: 9,
      title: "Existing",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/9",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      is_draft: false,
    });
    vi.mocked(ghViewPr).mockResolvedValue({
      number: 9,
      title: "Existing",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/9",
      body: "PR body",
      author: { login: "alice" },
      labels: [],
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      comments: null,
    });
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");

    const viewPr = await within(header).findByRole("button", {
      name: /view pr.*open/i,
    });
    expect(
      within(header).queryByRole("button", { name: /^create pr$/i }),
    ).not.toBeInTheDocument();

    await user.click(
      within(header).getByRole("button", { name: /open pr on web/i }),
    );
    expect(openUrl).toHaveBeenCalledWith("https://github.com/acme/treq/pull/9");
    vi.mocked(openUrl).mockClear();

    await user.click(viewPr);
    expect(openUrl).not.toHaveBeenCalled();
    expect(await screen.findByText("Existing")).toBeVisible();
    expect(ghViewPr).toHaveBeenCalledWith("acme/treq", 9);
  });

  it("exposes accessible names for the View PR controls", async () => {
    await setupPushedWorkspaceWithGitHub();
    vi.mocked(getCachedPrInfo).mockResolvedValue({
      number: 9,
      title: "Existing",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/9",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      is_draft: false,
    });
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    const viewPr = await within(header).findByRole("button", {
      name: /view pr.*open/i,
    });
    const openOnWeb = within(header).getByRole("button", {
      name: /open pr on web/i,
    });

    expect(viewPr).toHaveAttribute("aria-label", "View PR (#9, open)");
    expect(openOnWeb).toHaveAttribute("aria-label", "Open PR on web");
  });

  it("uses draft label when PR is a draft", async () => {
    await setupPushedWorkspaceWithGitHub();
    vi.mocked(getCachedPrInfo).mockResolvedValue({
      number: 12,
      title: "Draft PR",
      state: "OPEN",
      url: "https://github.com/acme/treq/pull/12",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      is_draft: true,
    });
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    expect(
      await within(header).findByRole("button", { name: /view pr.*draft/i }),
    ).toBeVisible();
  });

  it("names View PR with the closed and merged states", async () => {
    await setupPushedWorkspaceWithGitHub();
    vi.mocked(getCachedPrInfo).mockResolvedValue({
      number: 10,
      title: "Closed",
      state: "CLOSED",
      url: "https://github.com/acme/treq/pull/10",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: null,
    });
    const view = render(<Dashboard />);

    let header = await openWorkspace("feat/create-pr");
    expect(
      await within(header).findByRole("button", { name: /view pr.*closed/i }),
    ).toBeVisible();

    view.unmount();
    vi.mocked(getCachedPrInfo).mockResolvedValue({
      number: 11,
      title: "Merged",
      state: "MERGED",
      url: "https://github.com/acme/treq/pull/11",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: null,
    });
    render(<Dashboard />);
    header = await openWorkspace("feat/create-pr");
    expect(
      await within(header).findByRole("button", { name: /view pr.*merged/i }),
    ).toBeVisible();
  });
});
