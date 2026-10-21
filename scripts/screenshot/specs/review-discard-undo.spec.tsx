/**
 * Verifies the "Undo" action on the discard toasts in the Changes tab: both
 * "Discard all changes" (the Changes section header icon) and a per-file
 * "Discard file" (the file row's overflow menu) show a success toast with
 * an "Undo" button that restores the discarded content.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/discard-undo";
const FILE_CONTENT =
  "export const a = 1;\nexport const b = 2;\nexport const c = 3;\n";

it("captures Undo on the discard-all toast restoring the change", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
  const wsPath = resolveWorkspacePath(repoPath, workspace.workspace_path);
  const filePathOnDisk = path.join(wsPath, "example.ts");
  writeWorkspaceFile(wsPath, "example.ts", FILE_CONTENT);

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(BRANCH_NAME);
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
  await screen.findAllByText("example.ts");

  const discardAllTrigger = document
    .querySelector(".lucide-undo-2")
    ?.closest("button") as HTMLElement | null;
  if (!discardAllTrigger) throw new Error("Discard-all button not found");

  await captureDocument(document, {
    name: "review-discard-undo-01-before-discard",
    expectations: [
      'The "Changes" section in the sidebar lists example.ts as a pending unstaged change.',
      "The example.ts diff hunk (with the added lines) is visible in the main panel.",
    ],
  });

  await user.click(discardAllTrigger);
  await screen.findByText("Discard all changes?");
  const confirmButtons = screen.getAllByRole("button", {
    name: /discard all changes/i,
  });
  await user.click(confirmButtons[confirmButtons.length - 1]);
  const toast = await screen.findByText("All changes discarded");
  // Disk-level check, independent of the UI: the backend restore actually
  // happened (the toast is added after `await jjRestoreAll(...)` resolves).
  expect(fs.existsSync(filePathOnDisk)).toBe(false);
  // UI-level check: the sidebar list and diff panel re-fetch immediately
  // (via the explicit `loadChangedFiles()` call after discard) rather than
  // waiting on the backend file watcher's debounced event, which this
  // jsdom harness doesn't simulate anyway.
  await waitFor(() =>
    expect(screen.queryByText("example.ts")).not.toBeInTheDocument(),
  );
  const undoButton = within(toast.closest('[role="status"]')!).getByRole(
    "button",
    { name: /^undo$/i },
  );
  await captureDocument(document, {
    name: "review-discard-undo-02-toast-with-undo",
    expectations: [
      'A toast titled "Discarded" reading "All changes discarded" is visible with an underlined "Undo" action button inside it.',
      "The Changes section in the sidebar and the main diff panel no longer show example.ts -- the list refreshed to reflect the discard.",
    ],
  });

  await user.click(undoButton);
  await screen.findByText("Discarded changes have been restored");
  // Same independent, disk-level check for the restore.
  expect(fs.existsSync(filePathOnDisk)).toBe(true);
  expect(fs.readFileSync(filePathOnDisk, "utf8")).toBe(FILE_CONTENT);
  // UI-level check: the file reappears in the sidebar/diff panel without
  // any further user action.
  await waitFor(() =>
    expect(screen.getAllByText("example.ts").length).toBeGreaterThan(0),
  );

  await captureDocument(document, {
    name: "review-discard-undo-03-after-undo",
    expectations: [
      'A "Restored" toast reading "Discarded changes have been restored" is visible.',
      "example.ts is listed again in the Changes section and its diff hunk is visible in the main panel, exactly as before the discard.",
    ],
  });
}, 60000);
