import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { fireEvent } from "@testing-library/react";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

// NOTE: FileBrowser's per-line "Add comment" button is only mounted when JS
// hoveredLine state matches (virtualized rows, not CSS
// group-hover), which userEvent's visibility-checked hover doesn't reliably
// trigger in jsdom. Existing production tests for this exact widget
// (test/integration/workspace/code.test.tsx) use fireEvent.mouseOver +
// mouseEnter for this one step; every other interaction below uses userEvent.

// Verifies the FileBrowser's own review session: comments accumulate across
// multiple files (useFileBrowserReview) and drive the shared ReviewActionBar,
// instead of the old behavior of firing a new agent session per comment.
it("captures adding review comments across two files in the FileBrowser", async () => {
  const branchName = "feat/filebrowser-review-spec";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(
    workspacePath,
    "app.ts",
    "export function runApp() {\n  return 'ready';\n}\n",
  );
  writeWorkspaceFile(
    workspacePath,
    "helper.ts",
    "export const helper = true;\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(branchName);
  await user.click(await screen.findByRole("button", { name: "app.ts" }));
  const fileBrowser = within(await screen.findByTestId("file-browser"));

  await fileBrowser.findByText("runApp");
  await captureDocument(document, {
    name: "filebrowser-review-01-before",
    expectations: [
      "The FileBrowser is open showing app.ts's contents, with no review action bar visible.",
      "The file tree on the left lists app.ts and helper.ts with no comment badges.",
    ],
  });

  // Add a comment on the first line of app.ts
  const [firstLine] = await fileBrowser.findAllByTestId("code-line");
  fireEvent.mouseOver(firstLine);
  fireEvent.mouseEnter(firstLine);
  await user.click(await screen.findByTitle("Add comment"));
  const firstCommentBox =
    await screen.findByPlaceholderText("Add a comment...");
  await user.type(
    firstCommentBox,
    "Please double check this function signature.",
  );
  await user.click(screen.getByRole("button", { name: "Add Comment" }));

  await fileBrowser.findByText("1 comment pending");
  await fileBrowser.findByTestId("file-comment-card");
  await captureDocument(document, {
    name: "filebrowser-review-02-first-comment",
    expectations: [
      'The same header row as the "Back" button also reads "1 comment pending" with Discard and Finish review controls, all in one row.',
      'The comment "Please double check this function signature." renders inline as a card directly under the line it\'s attached to, like the Changes tab (not floating separately).',
      "app.ts's row in the file tree shows a small comment-count badge reading 1.",
    ],
  });

  // Switch to the second file and add a comment there too.
  await user.click(fileBrowser.getByRole("button", { name: /helper\.ts/ }));
  await fileBrowser.findByText("helper");

  const helperLines = await fileBrowser.findAllByTestId("code-line");
  fireEvent.mouseOver(helperLines[0]);
  fireEvent.mouseEnter(helperLines[0]);
  await user.click(await screen.findByTitle("Add comment"));
  const secondCommentBox =
    await screen.findByPlaceholderText("Add a comment...");
  await user.type(secondCommentBox, "Consider renaming this constant.");
  await user.click(screen.getByRole("button", { name: "Add Comment" }));

  await fileBrowser.findByText("2 comments pending");
  await fileBrowser.findByText("Consider renaming this constant.");
  await captureDocument(document, {
    name: "filebrowser-review-03-second-file-comment",
    expectations: [
      'The header row next to "Back" now reads "2 comments pending", proving comments accumulate across files in one session.',
      'The comment "Consider renaming this constant." renders inline as a card under helper.ts\'s first line.',
      "Both app.ts and helper.ts show a comment-count badge of 1 each in the file tree.",
    ],
  });

  // Delete the helper.ts comment via its inline card to prove the shared
  // FileCommentSection component (from the Changes tab) is fully wired, not
  // just rendering read-only.
  await user.click(screen.getByTitle("Delete comment"));
  await fileBrowser.findByText("1 comment pending");
  await captureDocument(document, {
    name: "filebrowser-review-05-after-delete",
    expectations: [
      'The header now reads "1 comment pending" after deleting the helper.ts comment via its inline card.',
      "helper.ts's comment-count badge is gone from the file tree; app.ts's badge of 1 remains.",
    ],
  });

  // Open the "Finish review" popover to confirm it surfaces the batched send controls.
  // NOTE: the screenshot harness rasterizes a jsdom-serialized DOM with no real
  // layout engine, so Radix Popover/floating-ui can't compute real anchor
  // coordinates here — the popover card renders detached from its trigger in
  // this capture even though it anchors correctly in the real running app.
  // Only its content (not position) is a meaningful claim below.
  await user.click(fileBrowser.getByRole("button", { name: /finish review/i }));
  await screen.findByText("Finish your review");
  await captureDocument(document, {
    name: "filebrowser-review-06-finish-popover",
    expectations: [
      'A "Finish your review" card is present, stating 1 comment will be submitted.',
      'The card shows "Copy", "Plan", and "Edit" buttons for sending the batched review.',
    ],
  });
}, 60000);
