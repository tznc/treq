/**
 * Reviews open a new agent terminal with fresh context by default, even when
 * an idle agent terminal is already open:
 * - "Start Review" always opens a new terminal.
 * - "Finish review" → Edit opens a new terminal unless "Send to idle agent if
 *   one is available" is ticked, in which case the idle terminal gets it.
 */

import * as React from "react";
import { expect, it, onTestFinished } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { installFakeAgents } from "../../../test/fake-agent";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow, startGpuiAgent } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/review-idle-agent";

const agentTerminalCount = () =>
	document.querySelectorAll('[data-terminal-id^="agent-"]').length;

/** Workspace with an uncommitted diff and one idle agent terminal open. */
async function setupWithIdleAgent() {
	onTestFinished(installFakeAgents());
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.branch_name === BRANCH_NAME,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"example.ts",
		"export const answer = 42;\n",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");

	await startGpuiAgent(BRANCH_NAME);
	await waitFor(() => expect(agentTerminalCount()).toBe(1));

	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");
	return user;
}

async function addCommentAndOpenFinishReview(
	user: ReturnType<typeof userEvent.setup>,
) {
	const addCommentBtns = await screen.findAllByRole("button", {
		name: /add comment/i,
	});
	await user.click(addCommentBtns[0]);
	await user.type(
		await screen.findByPlaceholderText(/add a comment/i),
		"Fix this",
	);
	await waitFor(async () => {
		const submitBtn = screen
			.getAllByRole("button", { name: /add comment/i })
			.find((btn) => btn.textContent?.trim() === "Add Comment");
		expect(submitBtn).toBeDefined();
		await user.click(submitBtn!);
	});
	await screen.findByText("Fix this");
	await user.click(
		await screen.findByRole("button", { name: /finish review/i }),
	);
	return (await screen.findByTestId("use-idle-agent")) as HTMLInputElement;
}

it("Start Review opens a new agent terminal even when one is idle", async () => {
	const user = await setupWithIdleAgent();

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-01-before-start-review",
		expectations: [
			'A "Start Review" button is visible in the Changes tab action bar.',
			"Exactly one agent terminal tab is open in the terminal pane.",
		],
	});

	await user.click(await screen.findByTestId("start-agent-review"));
	await waitFor(() => expect(agentTerminalCount()).toBe(2), {
		timeout: 10000,
	});

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-02-after-start-review",
		expectations: [
			"A second agent terminal tab has been added next to the first one.",
			"The new review terminal is the active one.",
		],
	});
}, 60000);

it("Edit opens a new agent terminal by default", async () => {
	const user = await setupWithIdleAgent();
	const checkbox = await addCommentAndOpenFinishReview(user);
	expect(checkbox.checked).toBe(false);

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-03-popover-default",
		expectations: [
			'The "Finish your review" popover is open.',
			'An unchecked "Send to idle agent if one is available" checkbox sits above the Copy, Plan and Edit buttons.',
		],
	});

	await user.click(await screen.findByRole("button", { name: /^edit$/i }));
	await waitFor(() => expect(agentTerminalCount()).toBe(2), {
		timeout: 10000,
	});

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-04-edit-new-terminal",
		expectations: [
			"The popover has closed.",
			"Two agent terminal tabs are open: the idle one and a new Code Review one.",
		],
	});
}, 60000);

it("Edit reuses the idle agent when the user opts in", async () => {
	const user = await setupWithIdleAgent();
	const checkbox = await addCommentAndOpenFinishReview(user);
	await user.click(checkbox);
	expect(checkbox.checked).toBe(true);

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-05-popover-opted-in",
		expectations: [
			'The "Send to idle agent if one is available" checkbox is checked.',
		],
	});

	await user.click(await screen.findByRole("button", { name: /^edit$/i }));
	await screen.findByText("Review sent to agent");
	expect(agentTerminalCount()).toBe(1);

	await captureDocument(document, {
		name: "review-idle-agent-opt-in-06-edit-reused-idle",
		expectations: [
			'A "Review sent to agent" toast is visible.',
			"Still only one agent terminal tab is open.",
		],
	});
}, 60000);
