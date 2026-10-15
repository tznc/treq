/**
 * Finish Review popover Copy control: visible "Copy"/"Copied" labels plus a
 * distinct "Copy review" accessible name so it is not confused with file-path
 * copy buttons.
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

const BRANCH_NAME = "feat/copy-review-popover";

it("captures Copy then Copied in the Finish Review popover", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "example.ts",
    "export const a = 1;\nexport const b = 2;\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(BRANCH_NAME);
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
  await screen.findAllByText("example.ts");

  const addCommentBtns = await screen.findAllByRole("button", {
    name: /add comment/i,
  });
  await user.click(addCommentBtns[0]);
  await user.type(
    await screen.findByPlaceholderText(/add a comment/i),
    "Test comment",
  );
  await waitFor(async () => {
    const btns = screen.getAllByRole("button", { name: /add comment/i });
    const submitBtn = btns.find((btn) => btn.textContent?.trim() === "Add Comment");
    expect(submitBtn).toBeDefined();
    await user.click(submitBtn!);
  });
  await screen.findByText("Test comment");

  await user.click(await screen.findByRole("button", { name: /finish review/i }));
  const copyButton = await screen.findByRole("button", { name: /^copy review$/i });
  expect(copyButton).toHaveTextContent("Copy");

  await captureDocument(document, {
    name: "review-copy-popover-01-copy",
    expectations: [
      'The Finish Review popover is open with heading "Finish your review".',
      'The left action button shows the visible label "Copy" (not "Copied").',
      'Plan and Edit buttons sit to the right of Copy.',
    ],
  });

  await user.click(copyButton);
  await screen.findByRole("button", { name: /^copied review$/i });

  await captureDocument(document, {
    name: "review-copy-popover-02-copied",
    expectations: [
      'The same popover is still open.',
      'The left action button now shows the visible label "Copied".',
      'Plan and Edit remain visible on the right.',
    ],
  });
}, 60000);
