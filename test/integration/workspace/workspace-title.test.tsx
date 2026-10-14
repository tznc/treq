// @include-parallel
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  updateWorkspace,
} from "../../../src/lib/api";
import { render, screen, within } from "../../test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../utils";

describe("ShowWorkspace - title", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  const openWorkspace = async (title: string, description?: string) => {
    const id = await createWorkspace(repoPath, "feat/title");
    await updateWorkspace(repoPath, id, undefined, title, description);
    render(<Dashboard />);
    await user.click(await findSidebarBranchElement("feat/title"));
    return screen.findByTestId("workspace-title");
  };

  it("shows the full title above the description in Code, without a header title", async () => {
    const title =
      "A long workspace title with enough detail to span multiple lines in a narrow workspace pane";
    const panel = await openWorkspace(title, "Description underneath");
    expect(within(panel).getByRole("heading", { name: title })).toBeVisible();
    const description = await screen.findByTestId("workspace-description");
    expect(
      panel.compareDocumentPosition(description) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      within(screen.getByTestId("show-workspace-header")).queryByText(title),
    ).not.toBeInTheDocument();
  });

  it("edits and persists a title even without a description", async () => {
    const panel = await openWorkspace("Original title");
    await user.click(
      within(panel).getByRole("button", { name: "Edit workspace title" }),
    );
    const input = within(panel).getByRole("textbox", {
      name: "Workspace title",
    });
    expect(input).toHaveValue("Original title");
    await user.clear(input);
    await user.type(input, "Updated title");
    await user.click(within(panel).getByRole("button", { name: "Save" }));
    expect(
      await within(panel).findByRole("heading", { name: "Updated title" }),
    ).toBeVisible();
    expect(
      (await getWorkspaces(repoPath)).find(
        (w) => w.branch_name === "feat/title",
      )?.title,
    ).toBe("Updated title");
  });

  it("discards title edits on Cancel and Escape", async () => {
    const panel = await openWorkspace("Keep this title");
    for (const cancelWithEscape of [false, true]) {
      await user.click(
        within(panel).getByRole("button", { name: "Edit workspace title" }),
      );
      await user.type(
        within(panel).getByRole("textbox", { name: "Workspace title" }),
        " changed",
      );
      if (cancelWithEscape) await user.keyboard("{Escape}");
      else
        await user.click(within(panel).getByRole("button", { name: "Cancel" }));
      expect(
        within(panel).getByRole("heading", { name: "Keep this title" }),
      ).toBeVisible();
    }
    expect(
      (await getWorkspaces(repoPath)).find(
        (w) => w.branch_name === "feat/title",
      )?.title,
    ).toBe("Keep this title");
  });
});
