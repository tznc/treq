// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import {
  createWorkspace,
  deleteWorkspace,
  getWorkspaces,
} from "../../src/lib/api";
import { invalidateQueries } from "../../src/lib/swr-cache";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import {
  clickGpuiRow,
  findGpuiRow,
  gpuiSnapshot,
  pickGpuiRowMenuItem,
  sendGpuiAction,
  waitForNoGpuiRow,
} from "../gpui-sidebar";

describe("leaving a workspace when workspaces are removed", () => {
  let repoPath: string;
  let alphaId: number;
  let betaId: number;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    alphaId = await createWorkspace(repoPath, "feat/alpha");
    betaId = await createWorkspace(repoPath, "feat/beta");
  });

  async function openWorkspace(branch: string) {
    await clickGpuiRow(branch);
    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        branch,
      ),
    );
  }

  it("keeps the open workspace when another one is archived", async () => {
    render(<Dashboard />);
    await openWorkspace("feat/beta");

    await pickGpuiRowMenuItem("feat/alpha", "Archive Workspace");

    await waitForNoGpuiRow("feat/alpha");
    expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
      "feat/beta",
    );
  });

  it("lands on the stack parent when the open workspace is archived", async () => {
    await createWorkspace(repoPath, "feat/child", "feat/alpha");
    render(<Dashboard />);
    await openWorkspace("feat/child");

    await pickGpuiRowMenuItem("feat/child", "Archive Workspace");

    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        "feat/alpha",
      ),
    );
  });

  it("leaves the open workspace when it is removed outside the app", async () => {
    render(<Dashboard />);
    await openWorkspace("feat/alpha");

    await deleteWorkspace(repoPath, alphaId);
    await invalidateQueries(["workspaces", repoPath]);

    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).not.toHaveTextContent(
        "feat/alpha",
      ),
    );
  });

  it("refreshes the sidebar for the archives that succeed when one fails", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation();
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (
        cmd === "archive_workspace" &&
        (args as { id: number }).id === betaId
      ) {
        throw new Error("archive refused");
      }
      return originalInvoke!(cmd, args);
    });

    try {
      render(<Dashboard />);
      await findGpuiRow("feat/alpha");
      await clickGpuiRow("feat/alpha", { toggle: true });
      await clickGpuiRow("feat/beta", { toggle: true });
      await waitFor(() => expect(gpuiSnapshot()?.bulkArchive).toBe(2));
      sendGpuiAction({ kind: "bulkArchive" });

      expect(await screen.findByText(/archive refused/)).toBeInTheDocument();
      await waitForNoGpuiRow("feat/alpha");
      expect(await findGpuiRow("feat/beta")).toBeTruthy();
      expect((await getWorkspaces(repoPath)).map((w) => w.id)).toEqual([
        betaId,
      ]);
    } finally {
      vi.mocked(invoke).mockImplementation(originalInvoke!);
    }
  });
});
