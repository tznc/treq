/**
 * Scoped filesystem refresh: only the visible view reloads after watcher-style
 * events (debounced into treq:refresh-workspace-changes). FileBrowser reloads
 * the open file and tree; Changes reloads the diff; overview without FileBrowser
 * does not flash "Refreshing...".
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
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { dispatchRefreshWorkspaceChanges } from "../../../src/lib/change-file-drag";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/scoped-fs-refresh";

it("refreshes only the visible FileBrowser and Changes views", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	const workspacePath = resolveWorkspacePath(repoPath, workspace.workspace_path);

	writeWorkspaceFile(
		workspacePath,
		"scoped.txt",
		"changes before line\n",
	);
	writeWorkspaceFile(
		workspacePath,
		"app.ts",
		"export const browserBefore = true;\n",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");

	// Overview without FileBrowser: scoped refresh should not spin "Refreshing..."
	expect(screen.queryByText("Refreshing...")).toBeNull();
	writeWorkspaceFile(workspacePath, "scoped.txt", "changes ignored on overview\n");
	dispatchRefreshWorkspaceChanges({
		workspaceId,
		changedPaths: [`${workspacePath}/scoped.txt`],
	});
	expect(screen.queryByText("Refreshing...")).toBeNull();
	expect(screen.queryByText("changes ignored on overview")).toBeNull();

	await captureDocument(document, {
		name: "scoped-fs-refresh-01-overview-no-refresh",
		expectations: [
			"The workspace overview is visible with no FileBrowser open.",
			"No 'Refreshing...' spinner appears in the workspace header row.",
			"The edited scoped.txt content is not shown because overview is not the Changes tab.",
		],
	});

	// FileBrowser: reload open file and tree entries for changed paths only.
	await user.click(await screen.findByRole("button", { name: "app.ts" }));
	const fileBrowser = within(await screen.findByTestId("file-browser"));
	await fileBrowser.findByText("browserBefore");

	await captureDocument(document, {
		name: "scoped-fs-refresh-02-filebrowser-before",
		expectations: [
			"FileBrowser shows app.ts with the line 'export const browserBefore = true;'.",
			"The file tree lists app.ts but not agent-added.ts yet.",
		],
	});

	writeWorkspaceFile(
		workspacePath,
		"app.ts",
		"export const browserAfter = true;\n",
	);
	writeWorkspaceFile(
		workspacePath,
		"agent-added.ts",
		"export const fromAgent = 1;\n",
	);
	dispatchRefreshWorkspaceChanges({
		workspaceId,
		changedPaths: [
			`${workspacePath}/app.ts`,
			`${workspacePath}/agent-added.ts`,
		],
	});

	await fileBrowser.findByText("browserAfter");
	expect(fileBrowser.queryByText("browserBefore")).toBeNull();
	await fileBrowser.findByText("agent-added.ts");

	await captureDocument(document, {
		name: "scoped-fs-refresh-03-filebrowser-after",
		expectations: [
			"FileBrowser now shows 'export const browserAfter = true;' in the code panel.",
			"The file tree includes agent-added.ts alongside app.ts.",
			"No global 'Refreshing...' spinner is visible in the header.",
		],
	});

	// Changes tab: reload diff for the open working-copy file list.
	await user.click(await screen.findByRole("button", { name: /back/i }));
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await screen.findAllByText("scoped.txt");
	await screen.findByText(/changes ignored on overview/);

	await captureDocument(document, {
		name: "scoped-fs-refresh-04-changes-before",
		expectations: [
			"The Changes tab lists scoped.txt in the sidebar file list.",
			"The diff shows the added line 'changes ignored on overview'.",
		],
	});

	writeWorkspaceFile(
		workspacePath,
		"scoped.txt",
		"changes ignored on overview\nchanges after refresh\n",
	);
	dispatchRefreshWorkspaceChanges({
		workspaceId,
		changedPaths: [`${workspacePath}/scoped.txt`],
	});
	await screen.findByText(/changes after refresh/);

	await captureDocument(document, {
		name: "scoped-fs-refresh-05-changes-after",
		expectations: [
			"The Changes tab diff for scoped.txt now includes a green added line 'changes after refresh'.",
			"The previous line 'changes ignored on overview' is still visible above it.",
		],
	});

	expect(screen.getByText(/changes after refresh/)).toBeInTheDocument();
}, 120_000);
