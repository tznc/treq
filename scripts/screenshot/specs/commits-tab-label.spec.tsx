/**
 * Commits tab adornment: grey/red count on workspaces; conflict icon on home.
 */

import * as React from "react";
import { expect, it } from "vitest";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	checkAndRebaseWorkspaces,
	createCommit,
	createWorkspace,
	ensureWorkspaceIndexed,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

function commitsCountPill(): HTMLElement {
	const tab = screen.getByRole("tab", { name: /^Commits/ });
	const pill = tab.querySelector('[data-testid="commits-tab-count"]');
	if (!(pill instanceof HTMLElement)) {
		throw new Error("Commits tab count pill not found");
	}
	return pill;
}

it("captures grey Commits count pill on a workspace", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, "feat/commits-pill");
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);
	writeWorkspaceFile(workspacePath, "a.txt", "one\n");
	await createCommit(repoPath, workspaceId, "first workspace commit");

	render(<Dashboard />);
	await clickGpuiRow("feat/commits-pill");
	await screen.findByTestId("show-workspace-header");

	await waitFor(() => {
		const pill = commitsCountPill();
		expect(pill).toHaveTextContent("1");
		expect(pill.className).toContain("bg-muted");
	});

	await captureDocument(document, {
		name: "commits-tab-label-01-grey-count",
		expectations: [
			'The Commits tab shows a grey number pill reading "1".',
			"The pill sits next to the Commits label in the tab bar.",
			"No conflict warning icon appears on the Commits tab.",
		],
	});
}, 60000);

it("captures red Commits count pill when the workspace has conflicts", async () => {
	const { repoPath, defaultBranch } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, "feat/commits-red");
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(workspacePath, "conflict.txt", "workspace side\n");
	await createCommit(repoPath, workspaceId, "workspace conflicting change");
	writeWorkspaceFile(repoPath, "conflict.txt", "main side\n");
	await createCommit(repoPath, null, "main conflicting change");
	await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
	await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

	render(<Dashboard />);
	await clickGpuiRow("feat/commits-red");
	await waitForGpuiConflict(workspaceId);

	await waitFor(() => {
		const pill = commitsCountPill();
		expect(pill.className).toContain("bg-destructive");
	});

	await captureDocument(document, {
		name: "commits-tab-label-02-red-conflict",
		expectations: [
			"The Commits tab shows a red number pill next to Commits.",
			"The workspace conflict indicator is visible in the sidebar.",
			"The pill uses destructive red styling, not grey.",
		],
	});
}, 60000);

it("captures home Commits tab without a count pill", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	render(<Dashboard />);
	await screen.findByTestId("show-workspace-header");

	await waitFor(() => {
		const tab = screen.getByRole("tab", { name: /^Commits/ });
		expect(tab.querySelector('[data-testid="commits-tab-count"]')).toBeNull();
		expect(
			tab.querySelector('[data-testid="commits-tab-conflict-icon"]'),
		).toBeNull();
	});

	await captureDocument(document, {
		name: "commits-tab-label-03-home-no-count",
		expectations: [
			"On the home repository, the Commits tab has no number pill.",
			"Code, Commits, and Changes tabs are visible in the tab bar.",
			"The home workspace header is showing (not a stacked feature branch).",
		],
	});
}, 60000);
