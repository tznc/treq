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

const homeRow = (repoPath: string) =>
  waitFor(() => {
    const row = document.querySelector(
      `[data-testid="home-repo-row"][data-repo-path="${CSS.escape(repoPath)}"]`,
    ) as HTMLElement | null;
    if (!row) throw new Error(`no home row for ${repoPath}`);
    return row;
  });

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

    await homeRow(mainPath);
    await homeRow(supportingPath);
    const sidebar = await screen.findByTestId("workspace-sidebar");
    await within(sidebar).findByText("feat/main-work");
    await within(sidebar).findByText("feat/supporting-work");
    expect(
      within(sidebar).getAllByTestId("repo-workspace-group-label"),
    ).toHaveLength(2);
  });

  it("stacks a workspace in the supporting repository when it is on screen", async () => {
    render(<Dashboard />);

    await user.click(await homeRow(supportingPath));
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
    const sidebar = await screen.findByTestId("workspace-sidebar");

    await user.click(await within(sidebar).findByText("feat/supporting-open"));
    const header = await screen.findByTestId("show-workspace-header");
    await within(header).findByText("feat/supporting-open");
    await waitFor(() =>
      expect(
        within(sidebar)
          .getByText("feat/supporting-open")
          .closest(".bg-primary\\/20"),
      ).toBeTruthy(),
    );

    await user.click(await homeRow(mainPath));
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
    const sidebar = await screen.findByTestId("workspace-sidebar");
    const workspaceRow = await within(sidebar).findByText(
      "feat/hidden-by-collapse",
    );

    const labels = within(sidebar).getAllByTestId("repo-workspace-group-label");
    await user.click(labels[1]);

    await waitFor(() => expect(workspaceRow).not.toBeVisible());
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
