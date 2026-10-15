/**
 * Visual QA: double-clicking a partially visible terminal should bring
 * that terminal fully into the horizontal terminal pane.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow, openGpuiShell, startGpuiAgent } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/terminal-dblclick-scroll";

it("double-clicking a halfway-scrolled terminal scrolls it fully into view", async () => {
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

	// Three terminals at 40% min-width each forces horizontal overflow.
	await startGpuiAgent(BRANCH_NAME);
	await openGpuiShell(BRANCH_NAME);
	await openGpuiShell(BRANCH_NAME);

	const shellPanels = await waitFor(() => {
		const els = document.querySelectorAll('[data-terminal-id^="shell-"]');
		expect(els.length).toBeGreaterThanOrEqual(2);
		return els;
	});

	// Maximize so the terminal bodies are fully on-screen; the behavior under
	// test is horizontal overflow within the pane, not vertical page clipping.
	await user.click(screen.getByLabelText(/Maximize terminal/i));
	await screen.findByLabelText(/Restore terminal/i);

	const targetShell = shellPanels[shellPanels.length - 1] as HTMLElement;
	const scrollContainer = screen.getByTestId("terminal-scroll-container");
	expect(scrollContainer.contains(targetShell)).toBe(true);

	await waitFor(() => {
		expect(
			document.querySelectorAll("[data-terminal-id]").length,
		).toBeGreaterThanOrEqual(3);
	});

	// Narrow viewport so the 3×40% min-width group clearly overflows.
	await captureDocument(document, {
		name: "terminal-dblclick-scroll-01-before",
		viewport: { width: 640, height: 900 },
		expectations: [
			"The maximized terminal pane shows Claude and Shell columns side by side.",
			"A terminal column is cut off at the right edge of the pane (horizontal overflow).",
			"The Claude 1 agent header is visible on the left side of the pane.",
		],
	});

	const scrollBy = vi.fn();
	scrollContainer.scrollBy = scrollBy;
	vi.spyOn(scrollContainer, "getBoundingClientRect").mockReturnValue({
		x: 0,
		y: 0,
		top: 0,
		left: 0,
		bottom: 200,
		right: 400,
		width: 400,
		height: 200,
		toJSON: () => ({}),
	});
	vi.spyOn(targetShell, "getBoundingClientRect").mockReturnValue({
		x: 250,
		y: 0,
		top: 0,
		left: 250,
		bottom: 200,
		right: 650,
		width: 400,
		height: 200,
		toJSON: () => ({}),
	});

	await user.dblClick(within(targetShell).getByText("Shell"));

	expect(scrollBy).toHaveBeenCalledWith({
		left: 250,
		behavior: "smooth",
	});

	// Chromium rasterization can't see jsdom scrollBy; scroll the target into
	// view at capture time so the after shot shows the intended result.
	await captureDocument(document, {
		name: "terminal-dblclick-scroll-02-after",
		viewport: { width: 640, height: 900 },
		scrollIntoView: `[data-terminal-id="${CSS.escape(targetShell.dataset.terminalId!)}"]`,
		expectations: [
			"The double-clicked Shell terminal column is fully visible in the pane.",
			"The Shell header text is fully readable and not clipped by either edge.",
		],
	});
}, 60000);
