import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import {
  createWorkspace,
  getWorkspaces,
  listStashes,
  stashWorkspaceChanges,
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("applies a stash onto an existing workspace via the Apply popover", async () => {
  const sourceBranch = "feat/stash-apply-src";
  const targetBranch = "feat/stash-apply-dst";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const sourceId = await createWorkspace(repoPath, sourceBranch);
  const targetId = await createWorkspace(repoPath, targetBranch);
  const source = (await getWorkspaces(repoPath)).find((w) => w.id === sourceId);
  const target = (await getWorkspaces(repoPath)).find((w) => w.id === targetId);
  if (!source || !target) throw new Error("workspaces missing");

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, source.workspace_path),
    "apply-me.txt",
    "applied from stash\n",
  );
  await stashWorkspaceChanges(repoPath, sourceId);
  expect(await listStashes(repoPath)).toHaveLength(1);

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(sourceBranch);
  await screen.findByTestId("show-workspace-header");

  await user.keyboard("{Meta>}k{/Meta}");
  const cmdk = await screen.findByTestId("modal");
  await user.type(
    within(cmdk).getByPlaceholderText(/type a command/i),
    "stash",
  );
  await user.click(await within(cmdk).findByText("View Stashed Changes"));

  const stashModal = await screen.findByTestId("stash-modal");
  await within(stashModal).findByTestId("stash-diff-pane");
  await user.click(await within(stashModal).findByTestId("stash-apply-button"));
  const applyPopover = await screen.findByTestId("stash-apply-popover");

  await captureDocument(document, {
    name: "stash-apply-existing-01-popover",
    expectations: [
      "Apply... popover lists Home repo, feat/stash-apply-src, and feat/stash-apply-dst.",
      "New workspace... is available in the popover.",
    ],
  });

  await user.click(
    await within(applyPopover).findByTestId(
      `stash-apply-option-${targetBranch}`,
    ),
  );

  await waitFor(() =>
    expect(screen.queryByTestId("stash-apply-popover")).not.toBeInTheDocument(),
  );

  // Dialog closes on backdrop click (custom Dialog has no Escape handler).
  const stashDialog = screen.getByTestId("stash-modal");
  const backdrop = stashDialog.parentElement?.previousElementSibling;
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
    expect(screen.getAllByText("apply-me.txt").length).toBeGreaterThan(0),
  );

  await captureDocument(document, {
    name: "stash-apply-existing-02-on-target",
    expectations: [
      "On feat/stash-apply-dst Changes tab, apply-me.txt is listed in Changes after the stash was applied.",
      "The working-copy diff shows the applied content.",
    ],
  });
}, 90000);
