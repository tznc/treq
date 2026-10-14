// @include-parallel
import * as React from "react";
import { execSync } from "node:child_process";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "../test-utils";
import {
  createTestRepo,
  openRepo,
  setWorkspaceTargetBranchRaw,
} from "../utils";
import * as api from "../../src/lib/api";
import {
  createWorkspace,
  getWorkspaces,
  updateWorkspace,
} from "../../src/lib/api";
import type { PrInfo } from "../../src/lib/api-types";
import { Dashboard } from "../../src/components/Dashboard";
import type { NativeMenuEntry } from "../../src/lib/native-context-menu";
import { waitFor } from "@testing-library/react";
import {
  clickGpuiHome,
  clickGpuiRow,
  findGpuiRow,
  gpuiRepo,
  gpuiSnapshot,
  gpuiTitles,
  openGpuiMenu,
  sendGpuiAction,
  stackGpui,
} from "../gpui-sidebar";

const findWorkspaceByBranchName = (
  workspaces: Awaited<ReturnType<typeof getWorkspaces>>,
  branchName: string,
) => workspaces.find((workspace) => workspace.branch_name === branchName);

function selectMenuItem(entries: NativeMenuEntry[], label: string) {
  const flat = entries.flatMap((e) => (e.kind === "submenu" ? e.items : [e]));
  const entry = flat.find((e) => e.kind === "item" && e.text === label);
  if (entry?.kind !== "item") throw new Error(`no "${label}" item`);
  act(() => entry.onSelect());
}

describe("Dashboard - workspace list", () => {
  let repoPath: string;
  let defaultBranch: string;

  beforeEach(async () => {
    ({ repoPath, defaultBranch } = createTestRepo(false));

    openRepo(repoPath);

    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
  }, 15_000);

  it("renders workspace sidebar elements correctly branch names in the sidebar", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");
    await findGpuiRow("feat/beta");
    await waitFor(() => expect(gpuiRepo()?.homeRef).toBe(defaultBranch));
    expect(gpuiRepo()?.homeRef).not.toBe("unknown");
  });

  it("draws the workspace list in the GPUI sidebar", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");

    const snapshot = gpuiSnapshot()!;
    expect(snapshot.repoName).toBe(repoPath.split("/").pop());
    expect(snapshot.repos).toHaveLength(1);
    expect(gpuiTitles()).toEqual(
      expect.arrayContaining(["feat/alpha", "feat/beta"]),
    );
  });

  it("shows detached HEAD short hash in home repo row instead of unknown", async () => {
    execSync('git commit --allow-empty -m "temp commit for detached test"', {
      cwd: repoPath,
    });
    const detachedCommit = execSync("git rev-parse HEAD~1", {
      cwd: repoPath,
      encoding: "utf8",
    }).trim();
    execSync(`git checkout ${detachedCommit}`, { cwd: repoPath });

    render(<Dashboard />);

    await waitFor(() => {
      const homeRef = gpuiRepo()?.homeRef ?? "";
      expect(homeRef).toMatch(/\b[0-9a-f]{12}\b/i);
      expect(homeRef.toLowerCase()).not.toContain("unknown");
    });
  });

  it("keeps sidebar populated when a workspace self-targets", async () => {
    const workspaces = await getWorkspaces(repoPath);
    const alphaWorkspace = findWorkspaceByBranchName(workspaces, "feat/alpha");
    expect(alphaWorkspace).toBeTruthy();

    setWorkspaceTargetBranchRaw(repoPath, alphaWorkspace!.id, "feat/alpha");

    render(<Dashboard />);

    await waitFor(() => expect(gpuiRepo()?.homeRef).toBe(defaultBranch));
    await findGpuiRow("feat/alpha");
    await findGpuiRow("feat/beta");
  });

  describe("context menu", () => {
    beforeEach(() => {
      vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    });

    it("should copy home repo relative and full paths from context menu", async () => {
      render(<Dashboard />);
      await findGpuiRow("feat/alpha");
      await waitFor(() => expect(gpuiRepo()?.homeRef).toBeTruthy());

      const menu = () => openGpuiMenu({ kind: "homeMenu", repoPath });

      selectMenuItem(await menu(), "Copy relative path");
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(".");

      selectMenuItem(await menu(), "Copy full path");
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(repoPath);
    });

    it("should open home repo in Finder when selecting Open in Finder", async () => {
      render(<Dashboard />);
      await findGpuiRow("feat/alpha");

      const entries = await openGpuiMenu({ kind: "homeMenu", repoPath });
      expect(entries).toContainEqual(
        expect.objectContaining({ kind: "submenu", text: "Open in..." }),
      );
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      vi.mocked(revealItemInDir).mockResolvedValue(undefined);
      selectMenuItem(entries, "Open in Finder");

      expect(revealItemInDir).toHaveBeenLastCalledWith(repoPath);
    });

    it("should copy workspace relative and full paths from context menu", async () => {
      const workspaces = await getWorkspaces(repoPath);
      const alphaWorkspace = findWorkspaceByBranchName(
        workspaces,
        "feat/alpha",
      )!;
      expect(alphaWorkspace).toBeTruthy();

      render(<Dashboard />);

      const alpha = await findGpuiRow("feat/alpha");
      const menu = () => openGpuiMenu({ kind: "workspaceMenu", id: alpha.id });

      selectMenuItem(await menu(), "Copy relative path");
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
        `.treq/workspaces/${alphaWorkspace.workspace_path}`,
      );

      selectMenuItem(await menu(), "Copy full path");
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
        `${repoPath}/.treq/workspaces/${alphaWorkspace.workspace_path}`,
      );
    });

    it("should copy the GitHub PR link from context menu when a PR exists", async () => {
      const openPr: PrInfo = {
        number: 42,
        title: "feat/alpha",
        state: "OPEN",
        url: "https://github.com/ziinc/treq/pull/42",
        head_ref_name: "feat/alpha",
        base_ref_name: "main",
        merge_state_status: "CLEAN",
      };
      const spy = vi
        .spyOn(api, "listCachedPrStatuses")
        .mockResolvedValue({ "feat/alpha": openPr });
      vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
      vi.spyOn(api, "stopPrStatusPolling").mockResolvedValue(undefined);

      render(<Dashboard />);

      const alpha = await findGpuiRow("feat/alpha");
      const menuLabels = async () =>
        (await openGpuiMenu({ kind: "workspaceMenu", id: alpha.id })).flatMap(
          (e) => (e.kind === "separator" ? [] : [e.text]),
        );
      await waitFor(async () =>
        expect(await menuLabels()).toContain("Copy link to GitHub PR"),
      );

      selectMenuItem(
        await openGpuiMenu({ kind: "workspaceMenu", id: alpha.id }),
        "Copy link to GitHub PR",
      );
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
        openPr.url,
      );

      spy.mockRestore();
    });

    it("should not show the copy PR link option when there is no PR", async () => {
      const spy = vi
        .spyOn(api, "listCachedPrStatuses")
        .mockResolvedValue({ "feat/alpha": null });
      vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
      vi.spyOn(api, "stopPrStatusPolling").mockResolvedValue(undefined);

      render(<Dashboard />);

      const alpha = await findGpuiRow("feat/alpha");
      expect(alpha.pr).toBeNull();

      const entries = await openGpuiMenu({
        kind: "workspaceMenu",
        id: alpha.id,
      });
      const labels = entries.flatMap((e) =>
        e.kind === "separator" ? [] : [e.text],
      );
      expect(labels).toContain("Copy branch name");
      expect(labels).not.toContain("Copy link to GitHub PR");

      spy.mockRestore();
    });

    it("should open workspace in Finder from context menu", async () => {
      const workspaces = await getWorkspaces(repoPath);
      const alphaWorkspace = findWorkspaceByBranchName(
        workspaces,
        "feat/alpha",
      )!;
      expect(alphaWorkspace).toBeTruthy();

      render(<Dashboard />);

      const alpha = await findGpuiRow("feat/alpha");
      const entries = await openGpuiMenu({
        kind: "workspaceMenu",
        id: alpha.id,
      });
      expect(entries).toContainEqual(
        expect.objectContaining({ kind: "submenu", text: "Open in..." }),
      );
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      vi.mocked(revealItemInDir).mockResolvedValue(undefined);
      selectMenuItem(entries, "Open in Finder");

      expect(revealItemInDir).toHaveBeenLastCalledWith(
        `${repoPath}/.treq/workspaces/${alphaWorkspace.workspace_path}`,
      );
    });
  });

  describe("multi-select workspaces", () => {
    it("should support multi-select interactions while keeping the main repository unselectable", async () => {
      render(<Dashboard />);
      await findGpuiRow("feat/alpha");
      await findGpuiRow("feat/beta");

      await clickGpuiRow("feat/alpha", { toggle: true });
      await clickGpuiRow("feat/beta", { toggle: true });
      await waitFor(() => expect(gpuiSnapshot()?.bulkArchive).toBe(2));

      await clickGpuiRow("feat/alpha", { toggle: true });
      await clickGpuiRow("feat/beta", { shift: true });
      await waitFor(async () => {
        expect((await findGpuiRow("feat/alpha")).selected).toBe(true);
        expect((await findGpuiRow("feat/beta")).selected).toBe(true);
      });

      await clickGpuiHome();
      await waitFor(() => expect(gpuiSnapshot()?.bulkArchive).toBeNull());
      expect(
        screen.queryByText(/delete.*workspaces?/i),
      ).not.toBeInTheDocument();
    });

    it("should show delete button when workspaces are selected", async () => {
      render(<Dashboard />);

      await clickGpuiRow("feat/alpha", { toggle: true });
      await clickGpuiRow("feat/beta", { toggle: true });

      await waitFor(() => expect(gpuiSnapshot()?.bulkArchive).toBe(2));
    });

    it("shows GitHub as a sidebar item below the home repo row", async () => {
      render(<Dashboard />);

      await waitFor(() => expect(gpuiRepo()?.homeRef).toBeTruthy());
      expect(gpuiSnapshot()?.github).toBe(false);

      sendGpuiAction({ kind: "openGithub" });
      expect(
        await screen.findByRole("heading", { name: /github/i }),
      ).toBeTruthy();
    });

    it("highlights only the GitHub sidebar item when GitHub is open", async () => {
      render(<Dashboard />);

      await waitFor(() => expect(gpuiRepo()?.homeSelected).toBe(true));
      expect(gpuiSnapshot()?.github).toBe(false);

      sendGpuiAction({ kind: "openGithub" });

      await screen.findByRole("heading", { name: /github/i });

      await waitFor(() => {
        expect(gpuiSnapshot()?.github).toBe(true);
        expect(gpuiRepo()?.homeSelected).toBe(false);
      });
    });

    it("keeps shift-click selection contiguous across the visible sidebar order", async () => {
      await createWorkspace(repoPath, "gumbo-notes");
      await createWorkspace(repoPath, "rubber-test-123");
      await createWorkspace(repoPath, "dduck-joke-readme");
      await createWorkspace(repoPath, "zebra-notes");

      const workspaces = await getWorkspaces(repoPath);
      const dduckWorkspace = findWorkspaceByBranchName(
        workspaces,
        "dduck-joke-readme",
      );
      const rubberWorkspace = findWorkspaceByBranchName(
        workspaces,
        "rubber-test-123",
      );
      const gumboWorkspace = findWorkspaceByBranchName(
        workspaces,
        "gumbo-notes",
      );
      const zebraWorkspace = findWorkspaceByBranchName(
        workspaces,
        "zebra-notes",
      );
      expect(dduckWorkspace).toBeTruthy();
      expect(rubberWorkspace).toBeTruthy();
      expect(gumboWorkspace).toBeTruthy();
      expect(zebraWorkspace).toBeTruthy();

      await updateWorkspace(
        repoPath,
        rubberWorkspace!.id,
        dduckWorkspace!.branch_name,
      );
      await updateWorkspace(
        repoPath,
        zebraWorkspace!.id,
        rubberWorkspace!.branch_name,
      );

      render(<Dashboard />);

      const expectedBranches = [
        "dduck-joke-readme",
        "rubber-test-123",
        "zebra-notes",
        "feat/alpha",
        "feat/beta",
        "gumbo-notes",
      ];

      await waitFor(() => {
        for (const branchName of expectedBranches) {
          expect(gpuiTitles()).toContain(branchName);
        }
      });

      const visibleOrder = gpuiTitles().filter((name) =>
        expectedBranches.includes(name),
      );
      expect(visibleOrder).toHaveLength(expectedBranches.length);

      const anchorBranch = "dduck-joke-readme";
      const targetBranch = "gumbo-notes";
      const start = gpuiTitles().indexOf(anchorBranch);
      const end = gpuiTitles().indexOf(targetBranch);
      const rangeCount = Math.abs(start - end) + 1;
      expect(rangeCount).toBeGreaterThan(1);

      await clickGpuiRow(anchorBranch, { toggle: true });
      await clickGpuiRow(targetBranch, { shift: true });

      await waitFor(() => {
        const titles = gpuiTitles();
        const lo = Math.min(
          titles.indexOf(anchorBranch),
          titles.indexOf(targetBranch),
        );
        const hi = Math.max(
          titles.indexOf(anchorBranch),
          titles.indexOf(targetBranch),
        );
        gpuiRepo()!.workspaces.forEach((row, index) => {
          if (index >= lo && index <= hi) expect(row.selected).toBe(true);
        });
        expect(gpuiSnapshot()?.bulkArchive).toBe(rangeCount);
      });
    });
  });

  describe("home repo action buttons", () => {
    it("keeps home action buttons visible when unselected", async () => {
      render(<Dashboard />);

      await clickGpuiRow("feat/alpha");

      await waitFor(async () =>
        expect((await findGpuiRow("feat/alpha")).selected).toBe(true),
      );
      expect(gpuiRepo()?.homeSelected).toBe(false);
      expect(gpuiSnapshot()?.agentUnavailable).toBeNull();
      expect(gpuiSnapshot()?.shellUnavailable).toBeNull();
    });

    it("opens the stack dialog for home when Stack is clicked", async () => {
      vi.spyOn(api, "jjGitFetchBackground").mockResolvedValue(undefined);

      render(<Dashboard />);
      await findGpuiRow("feat/alpha");

      await stackGpui(null);

      expect(await screen.findByText("Stack a new Workspace")).toBeVisible();
      expect(
        screen.getByText(
          "Create a new workspace from the current branch. Optionally move file changes.",
        ),
      ).toBeVisible();
    });
  });
});
