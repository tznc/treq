import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { setRepoSetting, setSetting } from "../../../src/lib/api";
import { captureDocument } from "../capture";

const PROMPT = "Add input validation to the signup form";

// A session started from the "Start a new agent session" dialog must pick up
// the repo's default model (over the app default), same as one started from
// the sidebar. The fake agent stands in for the real CLI.
it("starts a prompt-dialog session on the repo default model", async () => {
	const restoreAgents = installFakeAgents();

	try {
		const { repoPath } = createTestRepo(false);
		openRepo(repoPath);
		await setSetting("default_model", "sonnet");
		await setRepoSetting(repoPath, "default_model", "opus");

		const user = userEvent.setup();
		render(<Dashboard />);
		await screen.findByTestId("show-workspace-header");

		await user.keyboard("{Meta>}i{/Meta}");
		const dialog = (
			await screen.findByRole("heading", { name: "Start a new agent session" })
		).closest('[data-testid="modal"]') as HTMLElement;
		await user.type(
			within(dialog).getByPlaceholderText("Describe a task..."),
			PROMPT,
		);
		await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

		await waitFor(() =>
			expect(
				document.querySelector('[data-terminal-id^="agent-"]'),
			).not.toBeNull(),
		);
		await screen.findByRole("button", { name: /opus/i });
		// The fake agent prints the flags it was launched with, so this proves
		// the model reached the CLI, not just the header label.
		const pane = screen.getByTestId("workspace-terminal-pane");
		await within(pane).findByText("model: opus", {}, { timeout: 15000 });

		await captureDocument(document, {
			name: "agent-session-default-model-01-opus",
			expectations: [
				`An agent terminal pane is open with a tab titled "${PROMPT}".`,
				"The terminal body shows the fake agent's output, including the lines 'fake-agent: claude' and 'model: opus'.",
			],
		});
	} finally {
		restoreAgents();
	}
}, 120000);
