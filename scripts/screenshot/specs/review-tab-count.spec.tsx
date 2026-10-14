/**
 * Verifies the Changes tab badge shows the stable total of unique committed +
 * uncommitted files both before and after selecting the Changes tab (it must
 * not drop when ChangesDiffViewer mounts).
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
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/review-badge-count";

function getReviewBadgeCount(): number | null {
	const tab = screen.getByRole("tab", { name: /^Changes/ });
	const badge = within(tab).queryByTestId("review-change-count");
	if (!badge?.textContent) return null;
	return Number(badge.textContent);
}

it("captures Review badge showing total WC + committed count before and after select", async () => {
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

	writeWorkspaceFile(workspacePath, "committed-only.txt", "committed\n");
	await createCommit(repoPath, workspaceId, "commit committed-only file");
	writeWorkspaceFile(workspacePath, "uncommitted-only.txt", "working only\n");
	await createWorkspace(repoPath, "feat/clean-badge");

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");

	await waitFor(() => {
		expect(getReviewBadgeCount()).toBe(2);
	});

	await captureDocument(document, {
		name: "review-tab-count-01-code-tab",
		expectations: [
			"The Code tab is selected; the Changes tab badge shows the number 2.",
			"The badge sits next to the Changes label as a muted pill, not empty.",
		],
	});

	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await screen.findByTitle("uncommitted-only.txt");
	await screen.findByTitle("committed-only.txt");

	await waitFor(() => {
		expect(getReviewBadgeCount()).toBe(2);
	});

	await captureDocument(document, {
		name: "review-tab-count-02-review-tab",
		expectations: [
			"The Changes tab is selected and its badge still shows 2 (did not drop).",
			"The Changes sidebar lists uncommitted-only.txt and the Committed section lists committed-only.txt.",
		],
	});

	await clickGpuiRow("feat/clean-badge");
	await waitFor(() => {
		expect(getReviewBadgeCount()).toBeNull();
	});

	await captureDocument(document, {
		name: "review-tab-count-03-after-workspace-switch",
		expectations: [
			'The header shows the "feat/clean-badge" workspace.',
			"The Changes tab has no numeric badge — the previous workspace's count of 2 is gone.",
		],
	});
}, 60000);
