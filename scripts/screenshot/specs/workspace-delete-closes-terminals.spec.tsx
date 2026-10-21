/**
 * Regression spec: deleting a workspace must close the shell and agent
 * terminals tied to it (via WorkspaceTerminalPane.closeTerminalsForWorkspace),
 * instead of leaving them orphaned in the terminal pane.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ask } from "@tauri-apps/plugin-dialog";
import {
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow, openGpuiShell, startGpuiAgent } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/terminal-cleanup";

it("deleting a workspace closes its shell and agent terminals", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	await createWorkspace(repoPath, BRANCH_NAME);
	const workspaces = await getWorkspaces(repoPath);
	const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

	vi.mocked(ask).mockResolvedValue(true);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(workspace.branch_name);
	await screen.findByTestId("show-workspace-header");

	// Open an agent terminal and a shell terminal in this workspace.
	await startGpuiAgent(BRANCH_NAME);
	await waitFor(() => {
		expect(
			document.querySelector('[data-terminal-id^="agent-"]'),
		).not.toBeNull();
	});

	await openGpuiShell(BRANCH_NAME);
	await waitFor(() => {
		expect(
			document.querySelector('[data-terminal-id^="shell-"]'),
		).not.toBeNull();
	});

	await waitFor(() => {
		expect(
			document.querySelector('[data-terminal-id^="agent-"]'),
		).not.toBeNull();
		expect(
			document.querySelector('[data-terminal-id^="shell-"]'),
		).not.toBeNull();
	});

	await captureDocument(document, {
		name: "workspace-delete-closes-terminals-01-before",
		expectations: [
			"The bottom terminal pane is expanded with no full-width Terminals header bar.",
			"An agent terminal and a Shell terminal are visible; pane controls sit inset at the top-right.",
		],
	});

	// Delete the workspace via the header's "More workspace actions" menu.
	await user.click(
		await screen.findByRole("button", { name: "More workspace actions" }),
	);
	await user.click(await screen.findByText("Delete Workspace"));

	// The header stays mounted (Dashboard falls back to the main-repo view),
	// so assert on the deleted workspace's branch name disappearing instead.
	await waitFor(() => {
		expect(screen.queryByText(BRANCH_NAME)).not.toBeInTheDocument();
	});

	await waitFor(() => {
		expect(
			document.querySelector('[data-terminal-id^="agent-"]'),
		).toBeNull();
		expect(document.querySelector('[data-terminal-id^="shell-"]')).toBeNull();
	});

	await captureDocument(document, {
		name: "workspace-delete-closes-terminals-02-after",
		expectations: [
			"The app has navigated back to the main repository view, away from the deleted workspace.",
			"No terminal tabs for the deleted workspace remain visible anywhere on screen.",
		],
	});
}, 60000);
