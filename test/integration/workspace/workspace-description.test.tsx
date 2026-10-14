// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  updateWorkspace,
} from "../../../src/lib/api";
import { render, screen, within } from "../../test-utils";
import { createTestRepo, openRepo } from "../../utils";

describe("ShowWorkspace - description", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  const openWorkspaceWithDescription = async (description: string) => {
    const id = await createWorkspace(repoPath, "feat/desc");
    await updateWorkspace(repoPath, id, undefined, "Desc title", description);
    render(<Dashboard />);
    await clickGpuiRow("feat/desc");
    return screen.findByTestId("workspace-description");
  };

  it("renders a Markdown description in the Code tab", async () => {
    const panel = await openWorkspaceWithDescription(
      "## Summary\n\n- Rate-limit the login endpoint\n- Add a **retry** test",
    );

    expect(
      within(panel).getByRole("heading", { name: "Summary" }),
    ).toBeVisible();
    expect(within(panel).getAllByRole("listitem")).toHaveLength(2);
    expect(within(panel).queryByText(/##/)).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId("show-workspace-header")).queryByText(
        /Summary/,
      ),
    ).not.toBeInTheDocument();
  });

  it("renders a plain description as text, keeping its line breaks", async () => {
    const panel = await openWorkspaceWithDescription(
      "Fix the flaky login test.\nIt times out on CI.",
    );

    expect(panel).toHaveTextContent(
      "Fix the flaky login test. It times out on CI.",
    );
    expect(within(panel).queryByRole("heading")).not.toBeInTheDocument();
    expect(within(panel).getByText(/Fix the flaky/)).toHaveClass(
      "whitespace-pre-wrap",
    );
  });

  it("edits the description in place and saves it", async () => {
    const panel = await openWorkspaceWithDescription("Old task");

    await user.click(
      within(panel).getByRole("button", { name: "Edit workspace description" }),
    );
    const textarea = within(panel).getByRole("textbox", {
      name: "Workspace description",
    });
    expect(textarea).toHaveValue("Old task");
    await user.clear(textarea);
    await user.type(textarea, "## Plan{Enter}{Enter}- Ship it");
    await user.click(within(panel).getByRole("button", { name: "Save" }));

    expect(
      await within(panel).findByRole("heading", { name: "Plan" }),
    ).toBeVisible();
    expect(within(panel).queryByRole("textbox")).not.toBeInTheDocument();
    const [saved] = (await getWorkspaces(repoPath)).filter(
      (w) => w.branch_name === "feat/desc",
    );
    expect(saved.description).toBe("## Plan\n\n- Ship it");
  });

  it("discards edits on Cancel", async () => {
    const panel = await openWorkspaceWithDescription("Keep me");

    await user.click(
      within(panel).getByRole("button", { name: "Edit workspace description" }),
    );
    await user.type(
      within(panel).getByRole("textbox", { name: "Workspace description" }),
      " changed",
    );
    await user.click(within(panel).getByRole("button", { name: "Cancel" }));

    expect(panel).toHaveTextContent("Keep me");
    expect(panel).not.toHaveTextContent("changed");
    const [saved] = (await getWorkspaces(repoPath)).filter(
      (w) => w.branch_name === "feat/desc",
    );
    expect(saved.description).toBe("Keep me");
  });
});
