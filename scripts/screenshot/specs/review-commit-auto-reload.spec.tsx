/**
 * After committing while a Review is in progress (pending comments), the
 * Review diff panel must auto-reload instead of parking updates behind the
 * amber "file(s) changed since you started reviewing" banner.
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

const BRANCH_NAME = "feat/commit-auto-reload-qa";
const FILE_NAME = "commit-reload.txt";

it("auto-reloads the Review diff after commit instead of showing the stale banner", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		FILE_NAME,
		"first review line\nsecond review line\n",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await waitFor(() => {
		expect(screen.getAllByText(FILE_NAME).length).toBeGreaterThan(0);
	});

	await user.click(
		(await screen.findAllByRole("button", { name: /add comment/i }))[0],
	);
	await user.type(
		await screen.findByPlaceholderText(/add a comment/i),
		"Please tighten this change",
	);
	const submit = screen
		.getAllByRole("button", { name: /add comment/i })
		.find((button) => button.textContent === "Add Comment");
	expect(submit).toBeTruthy();
	await user.click(submit!);
	await screen.findByText("Please tighten this change");

	await captureDocument(document, {
		name: "review-commit-auto-reload-01-before-commit",
		expectations: [
			"The Review tab shows commit-reload.txt with an expanded green diff and a pending inline comment 'Please tighten this change'.",
			"No amber 'changed since you started reviewing' banner is visible above the diff panel.",
			"The left sidebar Changes section lists commit-reload.txt and a Commit button is available.",
		],
	});

	await user.type(
		screen.getByPlaceholderText("Message"),
		"Commit while reviewing",
	);
	await user.click(screen.getByRole("button", { name: /^commit$/i }));
	await screen.findByText("Commit created");

	await waitFor(() => {
		expect(
			screen.queryByText(/changed since you started reviewing/i),
		).toBeNull();
		expect(screen.getByTestId("committed-file-label")).toBeInTheDocument();
	});

	await captureDocument(document, {
		name: "review-commit-auto-reload-02-after-commit",
		expectations: [
			"After commit, no amber stale-files banner and no Reload button appear above the Review diff.",
			"The file shows a Committed label and the diff panel still displays the committed change without requiring a manual reload.",
			"A 'Commit created' toast confirms the commit succeeded.",
		],
	});
}, 60000);
