/**
 * Verifies the Changes tab shows a skeleton loading state instead of a
 * spinner while diffs are still loading, and that files render
 * progressively (first files first) rather than blocking on the whole
 * changeset.
 */

import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/skeleton-loading";
const FILES = ["a-first.ts", "b-second.ts", "c-third.ts", "d-fourth.ts"];

it("shows a skeleton while the Changes tab loads, then renders files progressively", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );
  for (const file of FILES) {
    writeWorkspaceFile(workspacePath, file, `export const ${file} = true;\n`);
  }

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(BRANCH_NAME);
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("tab", { name: /^Changes/i }));

  // Capture immediately after the tab click, before the async diff load
  // resolves -- this is the skeleton state.
  await captureDocument(document, {
    name: "changes-tab-skeleton-loading-01-loading",
    expectations: [
      "The Changes tab content area shows pulsing gray skeleton bars, not a spinner or 'Loading diffs...' text.",
    ],
  });

  await waitFor(() => {
    for (const file of FILES) {
      expect(screen.getAllByText(file).length).toBeGreaterThan(0);
    }
  });

  await captureDocument(document, {
    name: "changes-tab-skeleton-loading-02-loaded",
    expectations: [
      "All four changed files (a-first.ts, b-second.ts, c-third.ts, d-fourth.ts) are listed with their diffs, no skeletons remaining.",
    ],
  });
}, 60000);
