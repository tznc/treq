// @include-parallel
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import {
  addSupportingRepo,
  createWorkspace,
  getWorkspaces,
  listSupportingRepos,
} from "../../src/lib/api";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import {
  clickGpuiHome,
  clickGpuiRow,
  findGpuiRow,
  gpuiRepo,
  gpuiSnapshot,
  sendGpuiAction,
} from "../gpui-sidebar";

describe("Supporting repositories", () => {
  let mainPath: string;
  let supportingPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath: mainPath } = createTestRepo(false));
    const { repoPath: otherPath } = createTestRepo(false);
    supportingPath = await addSupportingRepo(mainPath, otherPath);
    openRepo(mainPath);
    user = userEvent.setup();
  });

  it("shows a home row and a workspace group for each repository", async () => {
    await createWorkspace(mainPath, "feat/main-work");
    await createWorkspace(supportingPath, "feat/supporting-work");
    render(<Dashboard />);

    await findGpuiRow("feat/main-work", { repoPath: mainPath });
    await findGpuiRow("feat/supporting-work", { repoPath: supportingPath });
    await waitFor(() => {
      const { repos } = gpuiSnapshot()!;
      expect(repos.map((r) => r.path)).toEqual([mainPath, supportingPath]);
      expect(repos[0].homeRef).toBeTruthy();
      expect(repos.every((r) => r.label)).toBe(true);
    });
  });

  it("stacks a workspace in the supporting repository when it is on screen", async () => {
    render(<Dashboard />);

    await clickGpuiHome(supportingPath);
    await screen.findByTestId("show-workspace-header");
    await user.click(await screen.findByRole("button", { name: "Stack" }));
    const dialog = await screen.findByTestId("modal");
    await user.type(within(dialog).getByLabelText("Branch Name"), "feat/svc");
    await user.click(
      within(dialog).getByRole("button", { name: "Create Workspace" }),
    );

    await waitFor(async () => {
      const branches = (await getWorkspaces(supportingPath)).map(
        (w) => w.branch_name,
      );
      expect(branches).toEqual(["feat/svc"]);
    });
    expect(await getWorkspaces(mainPath)).toEqual([]);
  });

  it("opens a supporting workspace and returns to the main home repository", async () => {
    await createWorkspace(supportingPath, "feat/supporting-open");
    render(<Dashboard />);

    await clickGpuiRow("feat/supporting-open", { repoPath: supportingPath });
    const header = await screen.findByTestId("show-workspace-header");
    await within(header).findByText("feat/supporting-open");
    await clickGpuiHome(mainPath);
    await waitFor(() =>
      expect(
        within(screen.getByTestId("show-workspace-header")).queryByText(
          "feat/supporting-open",
        ),
      ).toBeNull(),
    );
  });

  it("collapses a repository group", async () => {
    await createWorkspace(supportingPath, "feat/hidden-by-collapse");
    render(<Dashboard />);
    await findGpuiRow("feat/hidden-by-collapse", { repoPath: supportingPath });
    expect(gpuiRepo(supportingPath)?.collapsed).toBe(false);

    sendGpuiAction({ kind: "toggleRepo", repoPath: supportingPath });

    await waitFor(() => expect(gpuiRepo(supportingPath)?.collapsed).toBe(true));
    expect(gpuiRepo(mainPath)?.collapsed).toBe(false);
  });

  it("rejects adding the main repository to itself", async () => {
    await expect(addSupportingRepo(mainPath, mainPath)).rejects.toThrow(
      /main repository/,
    );
    expect((await listSupportingRepos(mainPath)).map((r) => r.path)).toEqual([
      supportingPath,
    ]);
  });
});
