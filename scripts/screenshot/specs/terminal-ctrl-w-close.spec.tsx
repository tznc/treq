/**
 * Captures the terminal-pane Close button tooltip showing the new "⌘ + W"
 * hotkey hint, and verifies that pressing Cmd+W while a terminal is
 * selected closes that terminal (rather than doing nothing, or closing the
 * whole treq window).
 */

import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/ctrl-w-close";

it("Cmd+W closes the selected terminal and shows the hotkey in the Close tooltip", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	await createWorkspace(repoPath, BRANCH_NAME);
	const workspaces = await getWorkspaces(repoPath);
	const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(workspace.branch_name);
	await screen.findByTestId("workspace-terminal-pane");

	// Create a shell terminal (Cmd+\) so there's a selected/active terminal to close.
	await user.keyboard("{Meta>}\\{/Meta}");

	const terminalPanel = await waitFor(() => {
		const el = document.querySelector('[data-terminal-id^="shell-"]');
		expect(el).not.toBeNull();
		return el as HTMLElement;
	});

	const closeButton = within(terminalPanel).getByLabelText(/close shell/i);

	await user.hover(closeButton);
	await waitFor(() => {
		expect(screen.getAllByText("Close").length).toBeGreaterThan(0);
	});
	await captureDocument(document, {
		name: "terminal-ctrl-w-close-01-tooltip",
		expectations: [
			'A tooltip is visible near the shell terminal\'s Close (X) button reading "Close" with a "⌘ + W" kbd badge to its right, matching the style of the other terminal-pane tooltips.',
		],
	});

	// Select the shell terminal (mirrors a real user clicking its header) so
	// it becomes the active/selected terminal before pressing Cmd+W.
	await user.click(within(terminalPanel).getByText("Shell"));

	// Press Cmd+W while the shell terminal is selected — it should close the
	// terminal, not the treq window.
	await user.keyboard("{Meta>}w{/Meta}");

	await waitFor(() => {
		expect(
			document.querySelector('[data-terminal-id^="shell-"]'),
		).toBeNull();
	});

	await captureDocument(document, {
		name: "terminal-ctrl-w-close-02-after-close",
		expectations: [
			"The shell terminal panel that was previously visible is gone; the workspace view underneath (not a closed/blank window) is still shown.",
		],
	});
}, 60000);
