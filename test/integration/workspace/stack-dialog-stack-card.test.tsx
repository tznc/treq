// @include-parallel
import { stackGpui } from "../../gpui-sidebar";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor, within } from "../../test-utils";
import { createTestRepo, openRepo } from "../../utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  setWorkspaceTargetBranch,
} from "../../../src/lib/api";
import { getFullWorkspacePath } from "../../../src/lib/utils";

const rowLabels = (card: HTMLElement) =>
  within(card)
    .getAllByRole("listitem")
    .map((item) => {
      const input = item.querySelector("input");
      return input ? "[new]" : (item.textContent ?? "").trim();
    });

describe("Stack dialog - stack card", () => {
  let repoPath: string;
  let defaultBranch: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("edits the branch name inside the new workspace row", async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await user.click(await screen.findByRole("button", { name: "Stack" }));
    const dialog = await screen.findByTestId("modal");

    const card = await within(dialog).findByTestId("new-workspace-stack-card");
    const newRow = within(card).getByTestId("new-workspace-stack-item");
    const input = within(newRow).getByLabelText("Branch Name");
    await user.type(input, "feat/typed-in-card");

    expect(input).toHaveValue("feat/typed-in-card");
    await waitFor(() => expect(rowLabels(card).at(-1)).toBe(defaultBranch));
  }, 15000);

  it("moves the new row below the parent when Before is chosen", async () => {
    await createWorkspace(repoPath, "feat/base");
    const parentId = await createWorkspace(repoPath, "feat/parent");
    const parent = (await getWorkspaces(repoPath)).find(
      (workspace) => workspace.id === parentId,
    );
    if (!parent) throw new Error("feat/parent workspace missing");
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(parent),
      parentId,
      "feat/base",
    );
    render(<Dashboard />);
    await stackGpui("feat/parent");
    const dialog = await screen.findByTestId("modal");
    const card = await within(dialog).findByTestId("new-workspace-stack-card");

    await waitFor(() =>
      expect(rowLabels(card)).toEqual([
        "[new]",
        "feat/parent",
        "feat/base",
        defaultBranch,
      ]),
    );

    await user.click(within(dialog).getByRole("button", { name: /^before$/i }));

    expect(rowLabels(card)).toEqual([
      "feat/parent",
      "[new]",
      "feat/base",
      defaultBranch,
    ]);
  }, 15000);
});
