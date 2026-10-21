import * as React from "react";
import { expect, it } from "vitest";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import {
	createCommit,
	createWorkspace,
	getWorkspaces,
} from "../../../src/lib/api";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("captures the Gerrit-style LOC marker on the workspace tab row", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const branchName = "feat/loc-screenshot";
	const workspaceId = await createWorkspace(repoPath, branchName);
	const workspace = (await getWorkspaces(repoPath)).find(
		(item) => item.id === workspaceId,
	);
	if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(
		workspacePath,
		"committed.txt",
		"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
	);
	await createCommit(repoPath, workspaceId, "add committed lines");

	writeWorkspaceFile(
		workspacePath,
		"working.txt",
		"alpha\nbeta\ngamma\n",
	);
	// Also delete a few lines from the committed file so -N is visible.
	writeWorkspaceFile(
		workspacePath,
		"committed.txt",
		"one\ntwo\nthree\nfour\nfive\n",
	);

	render(<Dashboard />);

	await clickGpuiRow(branchName);
	await screen.findByTestId("show-workspace-header");

	const marker = await screen.findByTestId("loc-diff-marker", {}, { timeout: 15000 });
	await waitFor(() => {
		expect(marker.textContent).toMatch(/\+\d+/);
		expect(marker.textContent).toMatch(/-\d+/);
	});

	await captureDocument(document, {
		name: "loc-marker-01-tab-row",
		clipSelector: "[data-testid='workspace-tab-row']",
		expectations: [
			"A Gerrit-style LOC marker sits on the right of the Code/Commits/Review tab row.",
			"The marker shows a green +N count, a thin centered green/red bar, and a red -N count.",
			"Both +N and -N are greater than zero for this workspace's committed and working-copy changes.",
		],
	});
}, 60000);
