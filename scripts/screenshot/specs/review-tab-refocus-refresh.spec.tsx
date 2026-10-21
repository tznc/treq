/**
 * Verifies that the Changes tab picks up on-disk file edits made while the
 * window was unfocused, once the window regains focus. This covers the case
 * where an already-modified file gets edited further: the file's jj status
 * doesn't change (it's still just "modified"/"added"), so the file-watcher
 * event and a plain file-list refresh both stay silent -- only re-fetching
 * hunk content on focus surfaces the new lines.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

const BRANCH_NAME = "feat/refocus-refresh";

it("refreshes the Changes tab's diff content when the window regains focus", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	const workspacePath = resolveWorkspacePath(repoPath, workspace.workspace_path);

	writeWorkspaceFile(workspacePath, "test.txt", "line one\nline two\n");

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("test.txt");
	await screen.findByText(/line two/);

	await captureDocument(document, {
		name: "review-tab-refocus-refresh-01-before-edit",
		expectations: [
			"The Changes tab shows test.txt's diff with two added lines: 'line one' and 'line two'.",
		],
	});

	// Simulate an external edit made while the window was unfocused. The
	// file was already uncommitted, so its jj status is unchanged -- this is
	// exactly the case a file-watcher-only refresh (or a file-list-only
	// refresh) would miss.
	writeWorkspaceFile(
		workspacePath,
		"test.txt",
		"line one\nline two\nline three\n",
	);

	// There's no real OS-level focus event to fire against jsdom/a Tauri-less
	// test process, so invoke the onFocusChanged listener the app registered
	// via getCurrentWindow().onFocusChanged directly, the same callback a
	// real window-focus event would invoke.
	const windowInstance = vi.mocked(getCurrentWindow).mock.results.at(-1)
		?.value;
	const focusCallback = vi
		.mocked(windowInstance.onFocusChanged)
		.mock.calls.at(-1)?.[0];
	if (!focusCallback) {
		throw new Error("onFocusChanged listener was not registered");
	}
	await focusCallback({ payload: true });

	await screen.findByText(/line three/);

	await captureDocument(document, {
		name: "review-tab-refocus-refresh-02-after-refocus",
		expectations: [
			"The Changes tab's test.txt diff now also shows 'line three' below 'line two', reflecting the on-disk edit made after the tab first loaded and the window regaining focus.",
		],
	});

	expect(screen.getByText(/line three/)).toBeInTheDocument();
}, 60000);
