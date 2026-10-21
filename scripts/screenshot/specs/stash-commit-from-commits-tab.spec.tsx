import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import {
  createWorkspace,
  getWorkspaces,
  listStashes,
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("stashes a commit from the Commits tab and applies it onto another workspace", async () => {
  const sourceBranch = "feat/stash-commit-src";
  const targetBranch = "feat/stash-commit-dst";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const sourceId = await createWorkspace(repoPath, sourceBranch);
  const targetId = await createWorkspace(repoPath, targetBranch);
  const source = (await getWorkspaces(repoPath)).find((w) => w.id === sourceId);
  const target = (await getWorkspaces(repoPath)).find((w) => w.id === targetId);
  if (!source || !target) throw new Error("workspaces missing");

  await commitWorkspaceFile(
    repoPath,
    { id: source.id, path: source.workspace_path },
    "parked-commit.txt",
    "parked from commits tab\n",
    "Park me for stash",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(sourceBranch);
  await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

  const commitHeadline = await screen.findByText("Park me for stash");
  await user.click(commitHeadline);
  const stashCommitBtn = await screen.findByTestId("stash-commit-button");

  await captureDocument(document, {
    name: "stash-commit-01-commits-tab-action",
    expectations: [
      "The Commits tab shows an expanded commit with a Stash commit button next to Move commit and Delete commit.",
      'The commit headline "Park me for stash" is visible.',
    ],
  });

  await user.click(stashCommitBtn);

  await waitFor(async () => {
    expect(await listStashes(repoPath)).toHaveLength(1);
  });

  const stashModal = await screen.findByTestId("stash-modal");
  await within(stashModal).findByTestId("stash-diff-pane");

  await captureDocument(document, {
    name: "stash-commit-02-stashed-modal",
    expectations: [
      "After Stash commit, the Stashed Changes dual-pane modal is open with the parked commit's diff.",
      "Apply... is available to move the stash onto another workspace.",
    ],
  });

  await user.click(await within(stashModal).findByTestId("stash-apply-button"));
  const applyPopover = await screen.findByTestId("stash-apply-popover");
  await user.click(
    await within(applyPopover).findByTestId(
      `stash-apply-option-${targetBranch}`,
    ),
  );

  await waitFor(() =>
    expect(screen.queryByTestId("stash-apply-popover")).not.toBeInTheDocument(),
  );

  const backdrop = stashModal.parentElement?.previousElementSibling;
  if (!(backdrop instanceof HTMLElement)) {
    throw new Error("expected stash dialog backdrop");
  }
  await user.click(backdrop);
  await waitFor(() =>
    expect(screen.queryByTestId("stash-modal")).not.toBeInTheDocument(),
  );

  await clickGpuiRow(targetBranch);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await waitFor(() =>
    expect(screen.getAllByText("parked-commit.txt").length).toBeGreaterThan(0),
  );

  await captureDocument(document, {
    name: "stash-commit-03-applied-on-target",
    expectations: [
      "On the target workspace Changes tab, parked-commit.txt appears after applying the stashed commit.",
      "A success toast about the stash apply may be visible.",
    ],
  });
}, 90000);
