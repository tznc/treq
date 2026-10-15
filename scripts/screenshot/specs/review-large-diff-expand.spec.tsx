/**
 * A file with many changed lines starts collapsed behind a "Large diff"
 * placeholder. Clicking "View changes" expands it in place, and the diff
 * viewer renders every line of it (the viewer renders eagerly, not in a
 * virtualized window -- see #440).
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

const BRANCH_NAME = "feat/large-diff";
const LINE_COUNT = 600;
const BIG_FILE = "big-file.ts";
const SECOND_FILE = "z-second-file.ts";

function manyLines(count: number): string {
  return Array.from({ length: count }, (_, i) => `line ${i}`).join("\n") + "\n";
}

function findDiffLine(filePath: string, lineIndex: number): HTMLElement | null {
  return document.querySelector(
    `[data-search-id="${filePath}:0:${lineIndex}"]`,
  );
}

it("expands a large diff from its placeholder and renders every line", async () => {
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
  writeWorkspaceFile(workspacePath, BIG_FILE, manyLines(LINE_COUNT));
  writeWorkspaceFile(workspacePath, SECOND_FILE, "export const done = true;\n");

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(BRANCH_NAME);
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
  await screen.findAllByText(BIG_FILE);

  const viewChangesButton = await screen.findByRole("button", {
    name: /view changes/i,
  });
  expect(findDiffLine(BIG_FILE, 0)).not.toBeInTheDocument();
  await captureDocument(document, {
    name: "review-large-diff-expand-01-collapsed-placeholder",
    expectations: [
      `The ${BIG_FILE} row shows a "Large diff" placeholder with a "View changes" button instead of the diff lines.`,
      `${SECOND_FILE} below it shows its one added line.`,
    ],
  });

  await user.click(viewChangesButton);
  await waitFor(() => {
    expect(findDiffLine(BIG_FILE, 0)).toBeInTheDocument();
  });
  expect(findDiffLine(BIG_FILE, LINE_COUNT - 1)).toBeInTheDocument();

  await captureDocument(document, {
    name: "review-large-diff-expand-02-expanded",
    expectations: [
      `The ${BIG_FILE} diff is expanded in place of the placeholder, showing green added lines starting from "line 0".`,
      'The "View changes" button is gone.',
    ],
  });
}, 60000);
