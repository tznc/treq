import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("captures stash-all from Changes and the dual-pane stash modal", async () => {
  const branchName = "feat/stash-demo";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "stash-demo.txt",
    "stashed line one\nstashed line two\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(branchName);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });

  await waitFor(() =>
    expect(screen.getAllByText("stash-demo.txt").length).toBeGreaterThan(0),
  );

  await captureDocument(document, {
    name: "stash-01-changes-before",
    expectations: [
      'The Changes section lists "stash-demo.txt" with a stash (archive) button available next to discard.',
      "No stash modal is open.",
    ],
  });

  const stashButton = await screen.findByTestId("stash-all-changes");
  await user.click(stashButton);

  await waitFor(() =>
    expect(screen.queryByText("stash-demo.txt")).not.toBeInTheDocument(),
  );

  await captureDocument(document, {
    name: "stash-02-after-stash-clean",
    expectations: [
      "After stashing, stash-demo.txt is gone from the Changes list (working copy cleaned).",
      "A success toast about stashed changes may be visible.",
    ],
  });

  // Open stash browser via command palette
  await user.keyboard("{Meta>}k{/Meta}");
  const cmdk = await screen.findByTestId("modal");
  const input = within(cmdk).getByPlaceholderText(/type a command/i);
  await user.type(input, "stash");
  await user.click(
    await within(cmdk).findByText("View Stashed Changes"),
  );

  const stashModal = await screen.findByTestId("stash-modal");
  const list = await within(stashModal).findByTestId("stash-list");
  expect(within(list).getByText(branchName)).toBeTruthy();
  await within(stashModal).findByTestId("stash-diff-pane");

  await captureDocument(document, {
    name: "stash-03-dual-pane-modal",
    expectations: [
      "A dual-pane 'Stashed Changes' dialog is open with a left list of stash entries and a right diff pane.",
      "The left list shows the workspace branch, a short commit hash, +/− LOC, and file count; the right pane shows the isolated stash-demo.txt diff.",
    ],
  });

  await user.click(await within(stashModal).findByTestId("stash-apply-button"));
  const applyPopover = await screen.findByTestId("stash-apply-popover");
  expect(within(applyPopover).getByText("Home repo")).toBeTruthy();
  expect(within(applyPopover).getByText(branchName)).toBeTruthy();
  expect(within(applyPopover).getByText("New workspace...")).toBeTruthy();

  await captureDocument(document, {
    name: "stash-04-apply-popover",
    expectations: [
      "The Apply... popover is open with a searchable list of workspaces (Home repo and feat/stash-demo) sorted for selection.",
      "A 'New workspace...' option is visible at the bottom of the apply popover.",
    ],
  });

  await user.click(
    await within(applyPopover).findByTestId("stash-apply-new-workspace"),
  );
  await waitFor(() =>
    expect(screen.queryByTestId("stash-modal")).not.toBeInTheDocument(),
  );

  // Create workspace dialog opens with the stash commit selected.
  await screen.findByRole("heading", { name: /stack a new workspace/i });
  await screen.findByTestId("stash-commit-selected");
  expect(
    screen.getByRole("button", { name: /create & apply stash/i }),
  ).toBeTruthy();

  await captureDocument(document, {
    name: "stash-05-create-with-stash-selected",
    expectations: [
      "The Create Workspace dialog is open with the right-hand Commits panel showing the stashed commit selected and labeled as stash.",
      "The primary submit button reads 'Create & apply stash'.",
    ],
  });
}, 90000);
