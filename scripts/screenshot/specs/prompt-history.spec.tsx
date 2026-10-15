import * as React from "react";
import { expect, it, onTestFinished } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { installFakeAgents } from "../../../test/fake-agent";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_A = "feat/prompt-history-demo";
const BRANCH_B = "feat/prompt-history-demo-2";
const PROMPT_A = "Build the login page and wire it to the auth API";
const PROMPT_B = "Add pagination to the results table";

async function submitPrompt(user: ReturnType<typeof userEvent.setup>, text: string) {
	const taskTextarea = await screen.findByPlaceholderText("Describe a task...");
	await user.type(taskTextarea, text);
	await user.click(await screen.findByRole("button", { name: /^edit$/i }));
	// Wait for the prompt to actually persist.
	await new Promise((resolve) => setTimeout(resolve, 500));
}

it("captures the workspace details starting prompt and the dual-pane prompt history modal", async () => {
	// Submitted prompts start agent terminals; give them an agent to run.
	onTestFinished(installFakeAgents());
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);
	await createWorkspace(repoPath, BRANCH_A);
	await createWorkspace(repoPath, BRANCH_B);

	const user = userEvent.setup();
	render(<Dashboard />);

	// Submit workspace A's prompt first, so it becomes the *older* of the two
	// -- this lets us prove "View full prompt" jumps straight to A even
	// though B (submitted after) is more recent and would otherwise be the
	// modal's default selection.
	await clickGpuiRow(BRANCH_A);
	await screen.findByTestId("show-workspace-header");
	await submitPrompt(user, PROMPT_A);

	await clickGpuiRow(BRANCH_B);
	await screen.findByTestId("show-workspace-header");
	await submitPrompt(user, PROMPT_B);

	// Back to workspace A: open Details and confirm its own starting prompt
	// (not B's, which was submitted later) shows up, with the max-height,
	// scrollable prompt box and the new "View full prompt" link.
	await clickGpuiRow(BRANCH_A);
	await screen.findByTestId("show-workspace-header");
	const detailsButton = await screen.findByTestId("workspace-details-button");
	await user.click(detailsButton);
	await screen.findByTestId("workspace-starting-prompt-text");

	await captureDocument(document, {
		name: "prompt-history-01-workspace-details-starting-prompt",
		expectations: [
			"An open popover titled 'Workspace details' shows the workspace's branch name and a 'Starting prompt' section.",
			`The Starting prompt section displays the text "${PROMPT_A}" with a Copy button next to the heading and a "View full prompt" link below the text.`,
		],
	});

	// Click "View full prompt" -- this should open the Prompt History modal
	// with workspace A's entry pre-selected in the right-hand detail pane,
	// even though it's not the most recently submitted prompt.
	await user.click(await screen.findByTestId("view-full-prompt-button"));
	await waitFor(() =>
		expect(
			screen.queryByTestId("workspace-details-popover"),
		).not.toBeInTheDocument(),
	);
	const promptHistoryModal = await screen.findByTestId("prompt-history-modal");
	const detailPane = await within(promptHistoryModal).findByTestId(
		"prompt-history-detail",
	);
	await within(detailPane).findByText(PROMPT_A);

	await captureDocument(document, {
		name: "prompt-history-02-dual-pane-jump-to-entry",
		expectations: [
			"A dual-pane 'Prompt History' dialog is open: a left list of prompt entries labeled by workspace (feat/prompt-history-demo and feat/prompt-history-demo-2), and a right detail pane.",
			`The right detail pane shows the FULL text "${PROMPT_A}" (workspace feat/prompt-history-demo's older prompt) even though a newer prompt exists, proving "View full prompt" jumped straight to that specific entry.`,
		],
	});

	// Clicking the other entry in the list should swap the detail pane over
	// to it, confirming the dual-pane selection is interactive.
	const listPane = within(promptHistoryModal).getByTestId("prompt-history-list");
	await user.click(within(listPane).getByText(PROMPT_B));
	await within(detailPane).findByText(PROMPT_B);

	await captureDocument(document, {
		name: "prompt-history-03-dual-pane-switch-selection",
		expectations: [
			`Clicking the feat/prompt-history-demo-2 entry in the left list now shows its full text "${PROMPT_B}" in the right detail pane.`,
			"The previously-selected list entry is no longer highlighted; the newly clicked entry is highlighted instead.",
		],
	});

	// "Run prompt..." on the currently-selected entry (B) should close the
	// history modal and open "Start a new agent session" with B's prompt
	// text pre-filled and B's workspace pre-selected in the combobox.
	await user.click(await screen.findByRole("button", { name: /run prompt/i }));
	await waitFor(() =>
		expect(screen.queryByTestId("prompt-history-modal")).not.toBeInTheDocument(),
	);
	await screen.findByRole("heading", { name: "Start a new agent session" });
	const agentPromptModal = await screen.findByTestId("modal");
	const prefilledTextarea = (await within(agentPromptModal).findByPlaceholderText(
		"Describe a task...",
	)) as HTMLTextAreaElement;
	await waitFor(() => expect(prefilledTextarea.value).toBe(PROMPT_B));

	await captureDocument(document, {
		name: "prompt-history-04-run-prompt-prefilled",
		expectations: [
			`The "Start a new agent session" dialog is open with its workspace combobox showing "${BRANCH_B}" and its prompt textarea pre-filled with the exact text "${PROMPT_B}".`,
		],
	});
}, 60000);
