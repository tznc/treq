// @include-parallel
import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getCachedPrInfo,
  getWorkspaces,
  ghCreatePr,
  getPrInfoViaGh,
  getWorkspaceStatus,
  pushWorkspaceToRemote,
  updateWorkspace,
} from "../../../src/lib/api";
import {
  clickGpuiToastAction,
  findGpuiToast,
  gpuiToasts,
} from "../../gpui-sidebar";
import { render, waitFor, within } from "../../test-utils";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
  setOriginUrl,
} from "../../utils";
import { deriveConventionalPrTitle } from "../../../src/lib/github-pr";

import {
  findEnabledCreatePr,
  openWorkspace as openWorkspaceAs,
  setupPushedWorkspaceWithGitHub as setupPushedWorkspace,
} from "./create-pr-helpers";
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
    ghCreatePr: vi.fn().mockResolvedValue(42),
    ghListPrs: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
    ghListIssues: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
    pushWorkspaceToRemote: vi.fn(
      (repoPath: string, workspaceId: number | null) =>
        original.pushWorkspaceToRemote(repoPath, workspaceId),
    ),
    getWorkspaceStatus: vi.fn(original.getWorkspaceStatus),
  };
});

describe("ShowWorkspace - Create PR", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
      "../../../src/lib/api",
    );
    vi.mocked(getWorkspaceStatus).mockImplementation(actual.getWorkspaceStatus);
    ({ repoPath } = createTestRepo(true));
    openRepo(repoPath);
    user = userEvent.setup();
    vi.mocked(getCachedPrInfo).mockReset().mockResolvedValue(null);
    vi.mocked(getPrInfoViaGh).mockReset().mockResolvedValue(null);
    vi.mocked(ghCreatePr).mockReset().mockResolvedValue(42);
    vi.mocked(openUrl).mockReset();
  });

  const setupPushedWorkspaceWithGitHub = (
    options?: Parameters<typeof setupPushedWorkspace>[1],
  ) => setupPushedWorkspace(repoPath, options);
  const openWorkspace = (branchName: string) =>
    openWorkspaceAs(user, branchName);

  it("shows Create PR instead of Push after the branch is on remote with GitHub", async () => {
    await setupPushedWorkspaceWithGitHub();
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");

    expect(
      await within(header).findByRole("button", { name: /^create pr$/i }),
    ).toBeVisible();
    expect(
      within(header).queryByRole("button", { name: /^push$/i }),
    ).not.toBeInTheDocument();
  });

  it("shows Create PR instead of Push when a GitHub remote is configured", async () => {
    await createWorkspace(repoPath, "feat/unpushed");
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
    render(<Dashboard />);

    const header = await openWorkspace("feat/unpushed");

    expect(
      await within(header).findByRole("button", { name: /^create pr$/i }),
    ).toBeVisible();
    expect(
      within(header).queryByRole("button", { name: /^push$/i }),
    ).not.toBeInTheDocument();
  });

  it("disables Create PR until the workspace has a real commit, not just working-copy changes", async () => {
    await createWorkspace(repoPath, "feat/no-commits");
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
    const view = render(<Dashboard />);

    const header = await openWorkspace("feat/no-commits");
    const createPr = await within(header).findByRole("button", {
      name: /^create pr$/i,
    });
    expect(createPr).toBeDisabled();
    expect(
      within(header).getByRole("button", { name: /more create pr options/i }),
    ).toBeDisabled();

    await user.click(createPr);
    expect(ghCreatePr).not.toHaveBeenCalled();

    view.unmount();

    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.branch_name === "feat/no-commits",
    )!;
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "feature.txt",
      "feature content",
      "Add feature",
    );

    render(<Dashboard />);
    const reopenedHeader = await openWorkspace("feat/no-commits");
    await findEnabledCreatePr(reopenedHeader);
  });

  it("keeps Push when the branch is not on remote and there's no GitHub remote", async () => {
    await createWorkspace(repoPath, "feat/unpushed");
    setOriginUrl(repoPath, "https://gitlab.com/acme/treq.git");
    render(<Dashboard />);

    const header = await openWorkspace("feat/unpushed");

    expect(
      await within(header).findByRole("button", { name: /^push$/i }),
    ).toBeVisible();
    expect(
      within(header).queryByRole("button", { name: /^create pr$/i }),
    ).not.toBeInTheDocument();
  });

  it("pushes the branch then creates the PR in one click when it isn't on remote yet", async () => {
    const workspaceId = await createWorkspace(repoPath, "feat/unpushed");
    await updateWorkspace(
      repoPath,
      workspaceId,
      undefined,
      "Ship the feature",
      "Implements the feature end-to-end.",
    );
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    )!;
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "feature.txt",
      "feature content",
      "Add feature",
    );
    vi.mocked(pushWorkspaceToRemote).mockResolvedValueOnce("pushed");
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
    render(<Dashboard />);

    const header = await openWorkspace("feat/unpushed");
    await user.click(await findEnabledCreatePr(header));

    await waitFor(() => {
      expect(pushWorkspaceToRemote).toHaveBeenCalledWith(repoPath, workspaceId);
    });
    await waitFor(() => {
      expect(ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        deriveConventionalPrTitle("Ship the feature", "feat/unpushed"),
        "Implements the feature end-to-end.",
        expect.any(String),
        "feat/unpushed",
        false,
      );
    });
    await findGpuiToast("Pull request created");
  });

  it("pushes committed changes before creating a PR when the branch is ahead of remote", async () => {
    const { workspace } = await setupPushedWorkspaceWithGitHub();
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "follow-up.txt",
      "unpushed content",
      "Add unpushed follow-up",
    );
    vi.mocked(pushWorkspaceToRemote).mockClear();
    vi.mocked(pushWorkspaceToRemote).mockResolvedValueOnce("pushed");
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await user.click(await findEnabledCreatePr(header));

    await waitFor(() => {
      expect(pushWorkspaceToRemote).toHaveBeenCalledWith(
        repoPath,
        workspace.id,
      );
    });
    await waitFor(() => {
      expect(ghCreatePr).toHaveBeenCalled();
    });
    expect(
      vi.mocked(pushWorkspaceToRemote).mock.invocationCallOrder[0],
    ).toBeLessThan(vi.mocked(ghCreatePr).mock.invocationCallOrder[0]);
    await findGpuiToast("Pull request created");
  });

  it("keeps Create PR disabled until the new PR's status has loaded", async () => {
    await setupPushedWorkspaceWithGitHub();
    let resolvePrInfo: () => void = () => {};
    vi.mocked(getPrInfoViaGh).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePrInfo = () => resolve(null);
        }),
    );
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await user.click(await findEnabledCreatePr(header));

    const pending = await within(header).findByRole("button", {
      name: /creating/i,
    });
    expect(pending).toBeDisabled();
    expect(gpuiToasts().map((t) => t.title)).not.toContain(
      "Pull request created",
    );

    resolvePrInfo();
    await findGpuiToast("Pull request created");
  }, 30_000);

  it.each([
    "CLOSED",
    "MERGED",
  ] as const)("offers Create PR again when the branch's last PR is %s", async (state) => {
    await setupPushedWorkspaceWithGitHub();
    const oldPr = {
      number: 7,
      title: "Old attempt",
      state,
      url: "https://github.com/acme/treq/pull/7",
      head_ref_name: "feat/create-pr",
      base_ref_name: "main",
      merge_state_status: null,
      is_draft: false,
    };
    vi.mocked(getCachedPrInfo).mockResolvedValue(oldPr);
    vi.mocked(getPrInfoViaGh).mockResolvedValue(oldPr);
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    expect(
      await within(header).findByRole("button", {
        name: new RegExp(`view pr.*${state}`, "i"),
      }),
    ).toBeVisible();
    await user.click(await findEnabledCreatePr(header));
    await findGpuiToast("Pull request created");
  }, 30_000);

  it("pushes a rewritten (diverged) branch before creating a PR", async () => {
    const { workspace } = await setupPushedWorkspaceWithGitHub();
    const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
      "../../../src/lib/api",
    );
    vi.mocked(getWorkspaceStatus).mockImplementation(async (...args) => ({
      ...(await actual.getWorkspaceStatus(...args)),
      remote_sync: { type: "Diverged", data: { ahead: 1, behind: 1 } },
    }));
    vi.mocked(pushWorkspaceToRemote).mockClear();
    vi.mocked(pushWorkspaceToRemote).mockResolvedValueOnce("pushed");
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await user.click(await findEnabledCreatePr(header));

    await findGpuiToast("Pull request created");
    expect(pushWorkspaceToRemote).toHaveBeenCalledWith(repoPath, workspace.id);
  }, 30_000);

  it("hides Create PR when there is no GitHub remote", async () => {
    await setupPushedWorkspaceWithGitHub({ githubRemote: false });
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");

    await waitFor(() => {
      expect(
        within(header).queryByRole("button", { name: /^push$/i }),
      ).not.toBeInTheDocument();
    });
    expect(
      within(header).queryByRole("button", { name: /^create pr$/i }),
    ).not.toBeInTheDocument();
  });

  it("creates a PR with the workspace title and description", async () => {
    const { title, description } = await setupPushedWorkspaceWithGitHub();
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await user.click(await findEnabledCreatePr(header));

    await waitFor(() => {
      expect(ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        deriveConventionalPrTitle(title, "feat/create-pr"),
        description,
        expect.any(String),
        "feat/create-pr",
        false,
      );
    });

    await clickGpuiToastAction(/open in web/i);
    expect(openUrl).toHaveBeenCalledWith(
      "https://github.com/acme/treq/pull/42",
    );
  });

  it("uses a description an agent updated after the workspace view loaded", async () => {
    const { workspace } = await setupPushedWorkspaceWithGitHub();
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    const createPr = await findEnabledCreatePr(header);
    const agentBody = "## Summary\n\n- Updated by the agent";
    await updateWorkspace(
      repoPath,
      workspace.id,
      undefined,
      undefined,
      agentBody,
    );
    await user.click(createPr);

    await waitFor(() => {
      expect(ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        expect.any(String),
        agentBody,
        expect.any(String),
        "feat/create-pr",
        false,
      );
    });
  });
});
