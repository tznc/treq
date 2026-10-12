// @include-parallel
import * as React from "react";
import * as fs from "node:fs";
import * as path from "node:path";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, within } from "../../test-utils";
import { createTestRepo, openRepo } from "../../utils";
import { Dashboard } from "../../../src/components/Dashboard";

describe("Stack dialog - move to workspace section", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    fs.writeFileSync(path.join(repoPath, "dirty.txt"), "dirty\n");
    user = userEvent.setup();
  });

  const openStackDialog = async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await user.click(await screen.findByRole("button", { name: "Stack" }));
    return screen.findByTestId("modal");
  };

  it("renders the move section collapsed by default", async () => {
    const dialog = await openStackDialog();

    const toggle = within(dialog).getByRole("button", {
      name: "Move to workspace",
    });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(dialog).queryByRole("tab", { name: /^Commits/ })).toBeNull();
    expect(within(dialog).queryByRole("tab", { name: /^Changes/ })).toBeNull();
  });

  it("shows the commits and changes tabs when expanded", async () => {
    const dialog = await openStackDialog();

    const toggle = within(dialog).getByRole("button", {
      name: "Move to workspace",
    });
    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(
      await within(dialog).findByRole("tab", { name: /^Commits/ }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("tab", { name: /^Changes/ }),
    ).toBeInTheDocument();
  });
});
