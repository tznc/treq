// @include-parallel
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import { pickGpuiRowMenuItem } from "../../gpui-sidebar";
import { createTestRepo, openRepo } from "../../utils";

async function openRenameDialog(user: ReturnType<typeof userEvent.setup>) {
  render(<Dashboard />);
  await pickGpuiRowMenuItem("feat/alpha", "Rename Workspace");
  const dialog = await screen.findByRole("dialog");
  const input = within(dialog).getByLabelText("Branch Name");
  await user.clear(input);
  return { dialog, input };
}

async function branchNames(repoPath: string) {
  return (await getWorkspaces(repoPath)).map((w) => w.branch_name);
}

describe("rename workspace dialog", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    user = userEvent.setup();
  });

  it("reports an invalid branch name and keeps Rename disabled", async () => {
    const { dialog, input } = await openRenameDialog(user);

    await user.type(input, "foo bar");

    expect(
      await within(dialog).findByText(/Invalid branch name/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "Rename" }),
    ).toBeDisabled();
  });

  it("does not submit a name that has not been checked yet", async () => {
    const { dialog, input } = await openRenameDialog(user);
    const rename = within(dialog).getByRole("button", { name: "Rename" });

    await user.type(input, "feat/ok");
    await waitFor(() => expect(rename).toBeEnabled());
    await user.type(input, "x");
    await user.keyboard("{Control>}{Enter}{/Control}");

    expect(rename).toBeDisabled();
    expect(await branchNames(repoPath)).toEqual(["feat/alpha"]);

    await waitFor(() => expect(rename).toBeEnabled());
    await user.click(rename);
    await waitFor(async () =>
      expect(await branchNames(repoPath)).toEqual(["feat/okx"]),
    );
  });

  it("checks and submits the trimmed name", async () => {
    const { dialog, input } = await openRenameDialog(user);
    const rename = within(dialog).getByRole("button", { name: "Rename" });

    await user.type(input, "  feat/beta  ");
    await waitFor(() => expect(rename).toBeEnabled());
    await user.click(rename);

    await waitFor(async () =>
      expect(await branchNames(repoPath)).toEqual(["feat/beta"]),
    );
  });
});
