/**
 * Verifies that "Send review to terminal" (the Plan/Edit buttons in the Finish
 * Review popover) respects the repo-level default_agent setting.  Before this
 * fix, a review was always sent to a Claude terminal even when the repo had
 * codex set as its default agent.  After the fix, clicking "Plan" should open
 * a Codex terminal (Codex brand icon) instead of a Claude terminal.
 */

import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, findSidebarWorkspaceRow, openRepo, resolveWorkspacePath, writeWorkspaceFile } from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createWorkspace,
	getWorkspaces,
	setRepoSetting,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/codex-review-agent";

it("send review to terminal opens a codex terminal when repo default_agent=codex", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	// Background state: set repo default agent to codex BEFORE the review is
	// submitted. handleCreateAgentWithReview reads this setting when the user
	// clicks Plan/Edit, so it just needs to be set before that click.
	await setRepoSetting(repoPath, "default_agent", "codex");

	// Create workspace via API (incidental — not the behavior under test).
	await createWorkspace(repoPath, BRANCH_NAME);
	const workspaces = await getWorkspaces(repoPath);
	const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

	// Write an uncommitted file so the Changes tab has a diff to show.
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"example.ts",
		'export const answer = 42;\n',
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	// Navigate to the workspace by clicking its name in the sidebar.
	await user.click(await screen.findByText(BRANCH_NAME));
	await screen.findByTestId("show-workspace-header");

	// Open an agent terminal before starting the review. Edit must not reuse
	// it: reviews open a new terminal unless the user opts into the idle agent.
	await user.click(
		within(await findSidebarWorkspaceRow(BRANCH_NAME)).getByRole("button", {
			name: "Start agent",
		}),
	);
	await waitFor(() => {
		expect(document.querySelector('[data-terminal-id^="agent-"]')).not.toBeNull();
	});

	// Open the Changes tab.
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	// The file appears in both sidebar and diff header — wait for any occurrence.
	await screen.findAllByText("example.ts");

	// The diff is already expanded; click "Add comment" on the first hunk line.
	// Use findAll to handle one button per diff line without ambiguity.
	const addCommentBtns = await screen.findAllByRole("button", {
		name: /add comment/i,
	});
	await user.click(addCommentBtns[0]);

	// Type a comment and submit it.
	await user.type(
		await screen.findByPlaceholderText(/add a comment/i),
		"Fix this",
	);
	// The textarea is inside a form that has a submit button also labelled
	// "Add Comment". Find the one with exact textContent to avoid ambiguity.
	await waitFor(async () => {
		const btns = screen.getAllByRole("button", { name: /add comment/i });
		const submitBtn = btns.find(
			(btn) => btn.textContent?.trim() === "Add Comment",
		);
		expect(submitBtn).toBeDefined();
		await user.click(submitBtn!);
	});
	await screen.findByText("Fix this");

	// Open the Finish Review popover.
	await user.click(await screen.findByRole("button", { name: /finish review/i }));

	await captureDocument(document, {
		name: "send-review-codex-01-popover",
		expectations: [
			'The "Finish review" popover is open showing the "Finish your review" heading.',
			'Three action buttons are visible: "Copy" on the left, "Plan" and "Edit" on the right.',
			'An unchecked "Send to idle agent if one is available" checkbox is above the buttons.',
		],
	});

	// Click "Edit" — this triggers handleCreateAgentWithReview which should
	// pick up default_agent=codex and pass agent:"codex" to onSessionCreated.
	await user.click(await screen.findByRole("button", { name: /^edit$/i }));

	// The review opens a second, Codex terminal next to the existing one.
	await waitFor(
		async () => {
			expect(
				document.querySelectorAll('[data-terminal-id^="agent-"]').length,
			).toBe(2);
		},
		{ timeout: 10000 },
	);

	await captureDocument(document, {
		name: "send-review-codex-02-terminal",
		expectations: [
			'The terminal pane at the bottom is open.',
			'A second agent terminal was created for the review next to the existing one.',
			'The new review terminal carries the Codex brand icon.',
		],
	});
}, 60000);
