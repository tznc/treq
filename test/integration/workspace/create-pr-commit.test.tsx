// @include-parallel
import { findGpuiToast, gpuiToasts } from "../../gpui-sidebar";
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
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  setOriginUrl,
  writeWorkspaceFile,
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

describe("ShowWorkspace - Commit and create PR", () => {
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

  it("updates the header to View PR after committing and creating a PR", async () => {
    const workspaceId = await createWorkspace(
      repoPath,
      "feat/commit-create-pr",
    );
    const workspace = (await getWorkspaces(repoPath)).find(
      (candidate) => candidate.id === workspaceId,
    )!;
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, workspace.workspace_path),
      "feature.txt",
      "feature content\n",
    );
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
    vi.mocked(pushWorkspaceToRemote).mockResolvedValueOnce("pushed");

    const createdPr = {
      number: 42,
      title: "Feature",
      state: "OPEN" as const,
      url: "https://github.com/acme/treq/pull/42",
      head_ref_name: "feat/commit-create-pr",
      base_ref_name: "main",
      merge_state_status: "CLEAN",
      is_draft: false,
    };
    vi.mocked(getPrInfoViaGh).mockImplementation(async () => {
      vi.mocked(getCachedPrInfo).mockResolvedValue(createdPr);
      return createdPr;
    });

    render(<Dashboard />);
    const header = await openWorkspace("feat/commit-create-pr");
    await user.click(await screen.findByRole("tab", { name: /changes/i }));
    await screen.findAllByText("feature.txt");
    await user.type(
      await screen.findByPlaceholderText("Message"),
      "Add feature",
    );
    await user.click(
      screen.getByRole("button", { name: /more commit options/i }),
    );
    const commitAndCreatePr = await screen.findByRole("menuitem", {
      name: /commit and create pr/i,
    });
    await waitFor(() => expect(commitAndCreatePr).toBeEnabled());
    await user.click(commitAndCreatePr);

    await waitFor(
      () => {
        expect(getPrInfoViaGh).toHaveBeenCalledWith(
          repoPath,
          "feat/commit-create-pr",
        );
      },
      { timeout: 15_000 },
    );
    expect(ghCreatePr).toHaveBeenCalledWith(
      "acme/treq",
      "feat: Add feature",
      expect.any(String),
      expect.any(String),
      "feat/commit-create-pr",
      false,
    );
    expect(
      await within(header).findByRole("button", { name: /view pr.*open/i }),
    ).toBeVisible();
    expect(
      within(header).queryByRole("button", { name: /^create pr$/i }),
    ).not.toBeInTheDocument();
  }, 30_000);

  it("reports a push failure after committing as a push error, not a PR error", async () => {
    const workspaceId = await createWorkspace(repoPath, "feat/push-fails");
    const workspace = (await getWorkspaces(repoPath)).find(
      (candidate) => candidate.id === workspaceId,
    )!;
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, workspace.workspace_path),
      "feature.txt",
      "feature content\n",
    );
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
    vi.mocked(pushWorkspaceToRemote).mockRejectedValueOnce(
      new Error("remote rejected"),
    );

    render(<Dashboard />);
    await openWorkspace("feat/push-fails");
    await user.click(await screen.findByRole("tab", { name: /changes/i }));
    await screen.findAllByText("feature.txt");
    await user.type(
      await screen.findByPlaceholderText("Message"),
      "Add feature",
    );
    await user.click(
      screen.getByRole("button", { name: /more commit options/i }),
    );
    const commitAndCreatePr = await screen.findByRole("menuitem", {
      name: /commit and create pr/i,
    });
    await waitFor(() => expect(commitAndCreatePr).toBeEnabled());
    await user.click(commitAndCreatePr);

    const toast = await findGpuiToast("Committed, but failed to push", {
      timeout: 15_000,
    });
    expect(toast.description).toBe("remote rejected");
    expect(gpuiToasts().map((t) => t.title)).not.toContain(
      "Failed to create PR",
    );
    expect(ghCreatePr).not.toHaveBeenCalled();
  }, 30_000);

  it("creates a draft PR from the dropdown", async () => {
    const { title, description } = await setupPushedWorkspaceWithGitHub();
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await findEnabledCreatePr(header);
    await user.click(
      within(header).getByRole("button", { name: /more create pr options/i }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /create draft pr/i }),
    );

    await waitFor(() => {
      expect(ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        deriveConventionalPrTitle(title, "feat/create-pr"),
        description,
        expect.any(String),
        "feat/create-pr",
        true,
      );
    });
  });

  it("opens GitHub compare URL when creating a PR manually", async () => {
    await setupPushedWorkspaceWithGitHub();
    render(<Dashboard />);

    const header = await openWorkspace("feat/create-pr");
    await findEnabledCreatePr(header);
    await user.click(
      within(header).getByRole("button", { name: /more create pr options/i }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /create pr manually/i }),
    );

    await waitFor(() => {
      expect(openUrl).toHaveBeenCalledWith(
        expect.stringContaining("https://github.com/acme/treq/compare/"),
      );
    });
    const url = vi.mocked(openUrl).mock.calls[0][0] as string;
    expect(url).toContain("feat%2Fcreate-pr");
    expect(url).toContain("title=feat%3A+Ship+the+feature");
    expect(url).toContain("body=Implements+the+feature+end-to-end.");
  });
});
