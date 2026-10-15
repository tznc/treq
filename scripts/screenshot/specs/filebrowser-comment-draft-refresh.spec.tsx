import * as React from "react";
import { expect, it } from "vitest";
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

// Per-line "Add comment" uses hoveredLine state; see filebrowser-review.spec.tsx.
it("captures a FileBrowser comment draft surviving a same-file reload", async () => {
  const branchName = "feat/filebrowser-comment-draft-refresh";
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);
  const workspacePath = resolveWorkspacePath(repoPath, workspace.workspace_path);
  writeWorkspaceFile(
    workspacePath,
    "notes.ts",
    "export const greeting = 'hello';\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(branchName);
  await user.click(await screen.findByRole("button", { name: "notes.ts" }));
  const fileBrowser = within(await screen.findByTestId("file-browser"));
  await fileBrowser.findByText("greeting");

  const [firstLine] = await fileBrowser.findAllByTestId("code-line");
  fireEvent.mouseOver(firstLine);
  fireEvent.mouseEnter(firstLine);
  await user.click(await screen.findByTitle("Add comment"));
  const commentBox = await screen.findByPlaceholderText("Add a comment...");
  await user.type(commentBox, "Keep this draft across reload");

  expect(commentBox).toHaveValue("Keep this draft across reload");
  await captureDocument(document, {
    name: "filebrowser-comment-draft-refresh-01-before-reload",
    expectations: [
      "An inline comment textarea is open under the first line of notes.ts.",
      'The textarea contains the draft "Keep this draft across reload".',
    ],
  });

  await user.click(fileBrowser.getByRole("button", { name: /notes\.ts/ }));
  const afterReload = await screen.findByPlaceholderText("Add a comment...");
  expect(afterReload).toHaveValue("Keep this draft across reload");
  await captureDocument(document, {
    name: "filebrowser-comment-draft-refresh-02-after-reload",
    expectations: [
      "The comment textarea is still open after clicking notes.ts in the file tree.",
      'The draft still reads "Keep this draft across reload".',
    ],
  });
}, 60000);
