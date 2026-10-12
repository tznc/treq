// @include-parallel
import * as React from "react";
import { execSync } from "node:child_process";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "../test-utils";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  findSidebarBranchElement,
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
import { waitFor, within } from "@testing-library/react";

const findWorkspaceByBranchName = (
  workspaces: Awaited<ReturnType<typeof getWorkspaces>>,
  branchName: string,
) => workspaces.find((workspace) => workspace.branch_name === branchName);

describe("Dashboard - workspace list", () => {
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));

    openRepo(repoPath);

    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
  }, 15_000);

  it("renders workspace sidebar elements correctly branch names in the sidebar", async () => {
    render(<Dashboard />);
    await screen.findByText("main");
    await screen.findByText("feat/alpha");
    await screen.findByText("feat/beta");
    expect(screen.queryByText("unknown")).toBeFalsy();
  });

  it("renders the workspace list inside a shadcn sidebar", async () => {
    render(<Dashboard />);
    await screen.findByText("feat/alpha");

    const sidebar = document.querySelector(
      '[data-sidebar="sidebar"]',
    ) as HTMLElement;
    expect(sidebar).toBeTruthy();
    expect(sidebar.querySelector('[data-sidebar="header"]')).toBeTruthy();
    expect(sidebar.querySelector('[data-sidebar="content"]')).toBeTruthy();
    expect(within(sidebar).getByText("Workspaces")).toBeTruthy();
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

    const sidebarRoot = document.querySelector(
      `.${CSS.escape("group/sidebar")}`,
    ) as HTMLElement;
    const homeRepoElement = sidebarRoot.querySelector(
      '[data-testid="home-repo-row"]',
    ) as HTMLElement;
    expect(homeRepoElement).toBeTruthy();
    await waitFor(() => {
      const homeText = homeRepoElement.textContent || "";
      expect(homeText).toMatch(/\b[0-9a-f]{12}\b/i);
      expect(homeText.toLowerCase()).not.toContain("unknown");
    });
  });

  it("keeps sidebar populated when a workspace self-targets", async () => {
    const workspaces = await getWorkspaces(repoPath);
    const alphaWorkspace = findWorkspaceByBranchName(workspaces, "feat/alpha");
    expect(alphaWorkspace).toBeTruthy();

    setWorkspaceTargetBranchRaw(repoPath, alphaWorkspace!.id, "feat/alpha");

    render(<Dashboard />);

    await screen.findByText("main");
    await screen.findByText("feat/alpha");
    await screen.findByText("feat/beta");
  });

  describe("context menu and tooltip", () => {
    let user: ReturnType<typeof userEvent.setup>;

    beforeEach(() => {
      user = userEvent.setup();
      vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    });

    it("should copy home repo relative and full paths from context menu", async () => {
      render(<Dashboard />);

      const matches = await screen.findAllByText("feat/alpha");
      expect(matches.length).toBeGreaterThan(0);

      const sidebarRoot = document.querySelector(
        `.${CSS.escape("group/sidebar")}`,
      );
      const homeRepoElement = sidebarRoot?.querySelector(
        '[data-testid="home-repo-row"]',
      ) as HTMLElement;
      expect(homeRepoElement).toBeTruthy();

      fireEvent.contextMenu(homeRepoElement);
      await screen.findByText("Copy relative path");
      await user.click(screen.getByText("Copy relative path"));
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(".");

      fireEvent.contextMenu(homeRepoElement);
      await screen.findByText("Copy full path");
      await user.click(screen.getByText("Copy full path"));
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(repoPath);
    });

    it("should open home repo in Finder when selecting Open in Finder", async () => {
      render(<Dashboard />);

      const matches = await screen.findAllByText("feat/alpha");
      expect(matches.length).toBeGreaterThan(0);

      const homeRepoElement = document.querySelector(
        '[data-testid="home-repo-row"]',
      ) as HTMLElement;
      expect(homeRepoElement).toBeTruthy();

      fireEvent.contextMenu(homeRepoElement!);

      await screen.findByText("Open in...");

      await user.hover(screen.getByText("Open in..."));

      await screen.findByText("Open in Finder");

      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      fireEvent.click(screen.getByText("Open in Finder"));

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

      const alphaElement = await findSidebarBranchElement("feat/alpha");

      fireEvent.contextMenu(alphaElement);
      await screen.findByText("Copy relative path");
      await user.click(screen.getByText("Copy relative path"));
      expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(
        `.treq/workspaces/${alphaWorkspace.workspace_path}`,
      );

      fireEvent.contextMenu(alphaElement);
      await screen.findByText("Copy full path");
      await user.click(screen.getByText("Copy full path"));
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

      const alphaElement = await findSidebarBranchElement("feat/alpha");

      fireEvent.contextMenu(alphaElement);
      await screen.findByText("Copy link to GitHub PR");
      await user.click(screen.getByText("Copy link to GitHub PR"));
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

      const alphaElement = await findSidebarBranchElement("feat/alpha");

      fireEvent.contextMenu(alphaElement);
      await screen.findByText("Copy branch name");
      expect(screen.queryByText("Copy link to GitHub PR")).toBeFalsy();

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

      const alphaElement = await findSidebarBranchElement("feat/alpha");

      fireEvent.contextMenu(alphaElement);

      await screen.findByText("Open in...");

      await user.hover(screen.getByText("Open in..."));

      await screen.findByText("Open in Finder");

      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      fireEvent.click(screen.getByText("Open in Finder"));

      expect(revealItemInDir).toHaveBeenLastCalledWith(
        `${repoPath}/.treq/workspaces/${alphaWorkspace.workspace_path}`,
      );
    });
  });

  describe("multi-select workspaces", () => {
    let user: ReturnType<typeof userEvent.setup>;

    beforeEach(() => {
      user = userEvent.setup();
    });

    it("should support multi-select interactions while keeping the main repository unselectable", async () => {
      const getSidebarElements = async () => {
        const alpha = await findSidebarBranchElement("feat/alpha");
        const beta = await findSidebarBranchElement("feat/beta");
        const sidebarRoot = document.querySelector(
          `.${CSS.escape("group/sidebar")}`,
        );
        const mainRepoRow = sidebarRoot!.querySelector(
          '[data-testid="home-repo-row"]',
        );

        return {
          workspace1: alpha.closest("div") as HTMLElement,
          workspace2: beta.closest("div") as HTMLElement,
          mainRepoRow: mainRepoRow as HTMLElement,
        };
      };

      const cmdClick = async (element: HTMLElement) => {
        await user.keyboard("{Meta>}");
        await user.click(element);
        await user.keyboard("{/Meta}");
      };

      const { rerender } = render(<Dashboard />);
      const { workspace1: cmdWorkspace1, workspace2: cmdWorkspace2 } =
        await getSidebarElements();

      await cmdClick(cmdWorkspace1);
      await cmdClick(cmdWorkspace2);

      rerender(<Dashboard />);
      const { workspace1: shiftWorkspace1, workspace2: shiftWorkspace2 } =
        await getSidebarElements();

      await cmdClick(shiftWorkspace1);
      await user.keyboard("{Shift>}");
      await user.click(shiftWorkspace2);
      await user.keyboard("{/Shift}");

      expect(shiftWorkspace1).toHaveClass("bg-primary/20");
      expect(shiftWorkspace2).toHaveClass("bg-primary/20");

      rerender(<Dashboard />);

      const { mainRepoRow } = await getSidebarElements();

      await cmdClick(mainRepoRow);

      expect(
        screen.queryByText(/delete.*workspaces?/i),
      ).not.toBeInTheDocument();
    });

    it("should show delete button when workspaces are selected", async () => {
      render(<Dashboard />);

      const alpha = await findSidebarBranchElement("feat/alpha");
      const beta = await findSidebarBranchElement("feat/beta");

      await user.keyboard("{Meta>}");
      await user.click(alpha);
      await user.click(beta);
      await user.keyboard("{/Meta}");

      await screen.findByText(/archive 2 workspaces/i);
    });

    it("shows GitHub as a sidebar item below the home repo row", async () => {
      render(<Dashboard />);

      const sidebarRoot = document.querySelector(
        `.${CSS.escape("group/sidebar")}`,
      ) as HTMLElement;
      expect(sidebarRoot).toBeTruthy();

      const homeRepoRow = await waitFor(() => {
        const row = sidebarRoot.querySelector(
          '[data-testid="home-repo-row"]',
        ) as HTMLElement;
        expect(row).toBeTruthy();
        return row;
      });

      const githubItem = within(sidebarRoot).getByRole("button", {
        name: /github/i,
      });
      expect(githubItem).toBeTruthy();
      expect(githubItem.textContent).toMatch(/github/i);

      const homeIndex = Array.from(sidebarRoot.querySelectorAll("*")).indexOf(
        homeRepoRow,
      );
      const githubIndex = Array.from(sidebarRoot.querySelectorAll("*")).indexOf(
        githubItem,
      );
      expect(githubIndex).toBeGreaterThan(homeIndex);

      await user.click(githubItem);
      expect(
        await screen.findByRole("heading", { name: /github/i }),
      ).toBeTruthy();
    });

    it("highlights only the GitHub sidebar item when GitHub is open", async () => {
      render(<Dashboard />);

      const sidebarRoot = document.querySelector(
        `.${CSS.escape("group/sidebar")}`,
      ) as HTMLElement;
      expect(sidebarRoot).toBeTruthy();

      const homeRepoRow = await waitFor(() => {
        const row = sidebarRoot.querySelector(
          '[data-testid="home-repo-row"]',
        ) as HTMLElement;
        expect(row).toBeTruthy();
        return row;
      });

      await waitFor(() => {
        expect(homeRepoRow).toHaveClass("bg-primary/20");
      });

      const githubItem = within(sidebarRoot).getByTestId("github-sidebar-item");
      await user.click(githubItem);

      await screen.findByRole("heading", { name: /github/i });

      await waitFor(() => {
        expect(githubItem).toHaveClass("bg-primary/20");
        expect(homeRepoRow).not.toHaveClass("bg-primary/20");
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

      const sidebarRoot = document.querySelector(
        `.${CSS.escape("group/sidebar")}`,
      ) as HTMLElement;

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
          expect(
            sidebarRoot.querySelector(
              `[data-testid="workspace-sidebar-item-${branchName}"]`,
            ),
          ).toBeTruthy();
        }
      });

      const getWorkspaceRow = (branchName: string) =>
        sidebarRoot.querySelector(
          `[data-testid="workspace-sidebar-item-${branchName}"]`,
        ) as HTMLElement;

      const visibleOrder = Array.from(
        sidebarRoot.querySelectorAll<HTMLElement>(
          "[data-testid^='workspace-sidebar-item-']",
        ),
      )
        .map(
          (el) =>
            el.dataset.testid?.replace("workspace-sidebar-item-", "") ?? "",
        )
        .filter((name) => expectedBranches.includes(name));

      expect(visibleOrder).toHaveLength(expectedBranches.length);

      const anchorBranch = "dduck-joke-readme";
      const targetBranch = "gumbo-notes";
      const anchorRow = getWorkspaceRow(anchorBranch);
      const targetRow = getWorkspaceRow(targetBranch);
      const rangeCount =
        Math.abs(
          Number(anchorRow.dataset.sidebarIndex) -
            Number(targetRow.dataset.sidebarIndex),
        ) + 1;
      expect(rangeCount).toBeGreaterThan(1);

      fireEvent.pointerDown(anchorRow, { button: 0, metaKey: true });
      fireEvent.click(anchorRow, { metaKey: true });
      fireEvent.pointerDown(targetRow, { button: 0, shiftKey: true });
      fireEvent.click(targetRow, { shiftKey: true });

      await waitFor(() => {
        const start = Math.min(
          Number(anchorRow.dataset.sidebarIndex),
          Number(targetRow.dataset.sidebarIndex),
        );
        const end = Math.max(
          Number(anchorRow.dataset.sidebarIndex),
          Number(targetRow.dataset.sidebarIndex),
        );
        for (const row of sidebarRoot.querySelectorAll<HTMLElement>(
          "[data-testid^='workspace-sidebar-item-']",
        )) {
          const index = Number(row.dataset.sidebarIndex);
          if (index >= start && index <= end) {
            expect(row).toHaveClass("bg-primary/20");
          }
        }
      });

      await screen.findByText(
        new RegExp(`archive ${rangeCount} workspaces`, "i"),
      );
    });
  });

  describe("home repo action buttons", () => {
    let user: ReturnType<typeof userEvent.setup>;

    beforeEach(() => {
      user = userEvent.setup();
    });

    it("keeps home action buttons visible when unselected", async () => {
      render(<Dashboard />);

      const homeRepoElement = await screen.findByTestId("home-repo-row");
      const alphaLabel = await findSidebarBranchElement("feat/alpha");
      const alphaRow = alphaLabel.closest("div") as HTMLElement;
      await user.click(alphaLabel);

      expect(alphaRow).toHaveClass("bg-primary/20");
      expect(homeRepoElement).not.toHaveClass("bg-primary/20");
      expect(
        within(homeRepoElement).getByRole("button", { name: "Start agent" }),
      ).toBeTruthy();
      expect(
        within(homeRepoElement).getByRole("button", { name: "Open shell" }),
      ).toBeTruthy();
      expect(
        within(homeRepoElement).getByRole("button", {
          name: "Stack a workspace",
        }),
      ).toBeTruthy();
    });

    it("opens the stack dialog for home when Stack is clicked", async () => {
      vi.spyOn(api, "jjGitFetchBackground").mockResolvedValue(undefined);

      render(<Dashboard />);

      const homeRepoElement = await screen.findByTestId("home-repo-row");
      await user.click(
        within(homeRepoElement).getByRole("button", {
          name: "Stack a workspace",
        }),
      );

      expect(await screen.findByText("Stack a new Workspace")).toBeVisible();
      expect(
        screen.getByText(
          "Create a new workspace from the current branch. Optionally move file changes.",
        ),
      ).toBeVisible();
    });
  });
});
