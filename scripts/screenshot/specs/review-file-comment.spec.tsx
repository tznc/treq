/**
 * Verifies the file-level "Add file comment" affordance in the Changes tab:
 * each file row's header has a comment button (like GitHub's per-file
 * comment), which opens a comment composer at the TOP of that file's
 * collapsible -- above the diff -- and the submitted comment renders there
 * as an editable card, distinct from line-anchored comments.
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
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/file-comment";

it("captures the file-level comment button and thread in the Changes tab", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	// Incidental background state: the workspace/file aren't what's under
	// test, the file-comment affordance rendered on top of them is.
	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"example.ts",
		"export const a = 1;\nexport const b = 2;\nexport const c = 3;\n",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	const commentButton = await screen.findByTestId("add-file-comment-button");
	await captureDocument(document, {
		name: "review-file-comment-01-button-in-header",
		expectations: [
			'The example.ts file row header shows a small speech-bubble-with-plus "Add file comment" icon button, sitting to the left of the green "Viewed" pill.',
			"The file's diff hunks are visible below the header (the collapsible is expanded), with no comment composer present yet.",
		],
	});

	await user.click(commentButton);
	const composer = await screen.findByPlaceholderText(/add a comment/i);
	await captureDocument(document, {
		name: "review-file-comment-02-composer-at-top",
		expectations: [
			'A comment composer with placeholder "Add a comment..." is now open at the very top of the example.ts collapsible, above the diff hunks (not anchored to any single line).',
			"The composer has no file:line label above the textarea, since this comment is scoped to the whole file rather than a line range.",
		],
	});

	await user.type(composer, "Please add a doc comment explaining these constants.");
	await user.click(
		screen.getAllByRole("button", { name: /^add comment$/i })[0],
	);
	await screen.findByTestId("file-comment-card");
	await screen.findByText(
		"Please add a doc comment explaining these constants.",
	);

	await captureDocument(document, {
		name: "review-file-comment-03-comment-added",
		expectations: [
			'A comment card reading "Please add a doc comment explaining these constants." sits at the top of the example.ts collapsible, above the diff hunks.',
			'The "Finish review" action bar at the top now reads "1 comment pending".',
		],
	});

	expect(screen.getByText(/1 comment pending/i)).toBeInTheDocument();

	// Click the card to edit it, matching the existing inline-comment
	// click-to-edit affordance.
	await user.click(await screen.findByTestId("file-comment-card"));
	const editTextarea = await screen.findByDisplayValue(
		"Please add a doc comment explaining these constants.",
	);
	await captureDocument(document, {
		name: "review-file-comment-04-edit-mode",
		expectations: [
			"The file comment card is now in edit mode: a textarea containing the comment text, with Discard, Cancel and Save buttons below it.",
		],
	});

	await user.clear(editTextarea);
	await user.type(editTextarea, "Please add JSDoc for each exported constant.");
	await user.click(screen.getByRole("button", { name: /^save$/i }));
	await screen.findByText("Please add JSDoc for each exported constant.");

	// Delete the comment via the card's X button; the section should
	// disappear entirely once no file comments remain.
	const card = await screen.findByTestId("file-comment-card");
	const deleteButton = await screen.findByTitle("Delete comment");
	await user.click(deleteButton);
	await screen.findByRole("tab", { name: /^Changes/i });
	expect(screen.queryByTestId("file-comment-card")).not.toBeInTheDocument();
	expect(card).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "review-file-comment-05-deleted",
		expectations: [
			"The file comment card and composer are both gone from the top of the example.ts collapsible -- only the diff hunks remain in the body.",
			'The "Finish review" action bar no longer shows a pending comment count (it is hidden, since there are no comments left).',
		],
	});
}, 60000);
