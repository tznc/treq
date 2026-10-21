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
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("captures the discard-all confirmation dialog in the Changes tab", async () => {
  const branchName = "feat/discard-all-test";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "discard-test.txt",
    "line one\nline two\nline three\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(branchName);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });

  await waitFor(() =>
    expect(screen.getAllByText("discard-test.txt").length).toBeGreaterThan(0),
  );

  await captureDocument(document, {
    name: "discard-all-confirmation-01-before",
    expectations: [
      'The "Changes" section lists "discard-test.txt".',
      "No confirmation dialog is open.",
    ],
  });

  const discardAllButton = document
    .querySelector(".lucide-undo-2")
    ?.closest("button");
  if (!discardAllButton) throw new Error("Discard-all button not found");
  await user.click(discardAllButton);

  await screen.findByText("Discard all changes?");
  await captureDocument(document, {
    name: "discard-all-confirmation-02-dialog-open",
    expectations: [
      'A confirmation dialog titled "Discard all changes?" is open, and it tells you that you can restore from the Undo button on the toast that follows.',
      'The dialog has a "Cancel" button and a red/destructive-colored "Discard all changes" button.',
    ],
  });

  const cancelButton = await screen.findByRole("button", { name: "Cancel" });
  await user.click(cancelButton);

  await waitFor(() =>
    expect(screen.queryByText("Discard all changes?")).not.toBeInTheDocument(),
  );
  await waitFor(() =>
    expect(screen.getAllByText("discard-test.txt").length).toBeGreaterThan(0),
  );
  await captureDocument(document, {
    name: "discard-all-confirmation-03-cancelled",
    expectations: [
      "The confirmation dialog has closed.",
      'The file "discard-test.txt" is still listed, unaffected by the cancelled discard.',
    ],
  });
}, 60000);
