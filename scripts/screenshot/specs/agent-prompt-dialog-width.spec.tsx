import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("captures the agent prompt dialog at cmdk-matching width", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);
	await createWorkspace(repoPath, "feat/agent-prompt-width");

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow("feat/agent-prompt-width");
	await screen.findByTestId("show-workspace-header");

	await user.keyboard("{Meta>}k{/Meta}");
	await user.click(
		await screen.findByText("Start a new agent session with a prompt"),
	);

	const promptDialog = (
		await screen.findByRole("heading", {
			name: "Start a new agent session",
		})
	).closest('[data-testid="modal"]');
	expect(promptDialog).toBeTruthy();
	expect(promptDialog?.className).toContain("w-[40vw]");
	expect(promptDialog?.className).toContain("max-w-none");

	await captureDocument(document, {
		name: "agent-prompt-dialog-01-open",
		expectations: [
			"The agent prompt dialog is centered on screen and noticeably wider than a compact modal — roughly 40% of the viewport width, matching the command palette.",
			"The dialog shows the title 'Start a new agent session', a workspace branch picker, and a task input area below.",
		],
	});
}, 60000);

it("captures the agent prompt dialog opened with Cmd+I on the home repo", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	await screen.findByTestId("show-workspace-header");
	await user.keyboard("{Meta>}i{/Meta}");

	const promptDialog = (
		await screen.findByRole("heading", {
			name: "Start a new agent session",
		})
	).closest('[data-testid="modal"]');
	expect(promptDialog).toBeTruthy();
	const workspacePicker = screen
		.getByRole("heading", { name: "Start a new agent session" })
		.closest('[data-testid="modal"]');
	expect(workspacePicker).toBeTruthy();
	expect(
		Array.from(workspacePicker?.querySelectorAll("button") ?? []).some(
			(button) => /branch-/.test(button.textContent ?? ""),
		),
	).toBe(true);

	await captureDocument(document, {
		name: "agent-prompt-dialog-02-cmd-i-home-repo",
		expectations: [
			"Cmd+I opens a centered modal titled 'Start a new agent session'.",
			"The repository picker displays the home repository's current branch by default.",
			"The task input area is visible below the repository picker.",
		],
	});
}, 60000);
