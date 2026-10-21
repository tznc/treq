/**
 * Verifies Review-tab Discard all actually clears uncommitted changes from
 * the UI and leaves the Changes section empty.
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
import fs from "node:fs";
import path from "node:path";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/discard-qa";
const FILE_NAME = "discard-me.txt";

it("captures Changes tab discard-all clearing the changes list", async () => {
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
	writeWorkspaceFile(workspacePath, FILE_NAME, "content to discard\n");

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText(FILE_NAME);

	await captureDocument(document, {
		name: "review-discard-01-before",
		expectations: [
			"The Changes tab is selected and the Changes sidebar lists discard-me.txt as an uncommitted file.",
			'A discard-all control (undo icon with accessible name "Discard all changes") is visible in the Changes section header.',
			"The main pane shows the discard-me.txt diff with the added dirty content.",
		],
	});

	await user.click(
		await screen.findByRole("button", { name: /discard all changes/i }),
	);
	await screen.findByText("Discard all changes?");
	const confirmButtons = screen.getAllByRole("button", {
		name: /discard all changes/i,
	});
	await user.click(confirmButtons[confirmButtons.length - 1]);

	await waitFor(() => {
		expect(screen.queryAllByText(FILE_NAME)).toHaveLength(0);
	});
	expect(fs.existsSync(path.join(workspacePath, FILE_NAME))).toBe(false);

	await captureDocument(document, {
		name: "review-discard-02-after",
		expectations: [
			"After discard, discard-me.txt is gone from the Changes sidebar and the Review badge count no longer shows that file.",
			'A success toast titled "Discarded" is visible confirming the changes were removed.',
			"The Review main pane no longer shows the discard-me.txt diff.",
		],
	});
}, 60000);
