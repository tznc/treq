// @include-parallel
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, updateWorkspace } from "../../../src/lib/api";
import { render, screen, within } from "../../test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../utils";

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
    await user.click(await findSidebarBranchElement("feat/desc"));
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
});
