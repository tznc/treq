/**
 * Verifies starting an agent prompt from a GitHub issue: the issue detail
 * page exposes "Agent...", and clicking it opens the agent prompt
 * dialog with the GitHub issue attached as a chip, like every tracker.
 */

import fs from "node:fs";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { ghListIssues, ghViewIssue } from "../../../src/lib/api";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

vi.mock("../../../src/lib/api", async (importOriginal) => {
	const original =
		await importOriginal<typeof import("../../../src/lib/api")>();
	return {
		...original,
		getCachedPrInfo: vi.fn().mockResolvedValue(null),
		startPrStatusPolling: vi.fn(async () => undefined),
		refreshPrBranchStatus: vi.fn(async () => undefined),
		stopPrStatusPolling: vi.fn(async () => undefined),
		refreshPrStatuses: vi.fn(async () => undefined),
		ghListPrs: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
		ghListIssues: vi.fn(),
		ghViewIssue: vi.fn(),
	};
});

function setOriginUrl(repoPath: string, remoteUrl: string) {
	const configPath = path.join(repoPath, ".git", "config");
	let config = fs.readFileSync(configPath, "utf-8");
	if (/\[remote "origin"\][\s\S]*?url\s*=/.test(config)) {
		config = config.replace(
			/(\[remote "origin"\][\s\S]*?url\s*=\s*).*/m,
			`$1${remoteUrl}`,
		);
	} else {
		config += `\n[remote "origin"]\n\turl = ${remoteUrl}\n`;
	}
	fs.writeFileSync(configPath, config);
}

const ISSUE = {
	number: 42,
	title: "Fix the login redirect",
	state: "OPEN",
	url: "https://github.com/acme/treq/issues/42",
	body: "Users land on /dashboard instead of /home after login.",
	author: { login: "alice", avatar_url: null },
	labels: [],
	created_at: "2026-01-01T00:00:00Z",
	updated_at: "2026-01-01T00:00:00Z",
	comments: null,
};

it("opens the agent prompt dialog with a GitHub issue chip from issue detail", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);
	setOriginUrl(repoPath, "https://github.com/acme/treq.git");

	vi.mocked(ghListIssues).mockResolvedValue({
		items: [ISSUE],
		hasMore: false,
	});
	vi.mocked(ghViewIssue).mockResolvedValue(ISSUE);

	const user = userEvent.setup();
	render(<Dashboard />);

	sendGpuiAction({ kind: "openGithub" });
	await screen.findByText("Fix the login redirect");
	await user.click(await screen.findByText("Fix the login redirect"));
	await screen.findByText("Issue #42");

	const agentButton = await screen.findByRole("button", {
		name: /^agent/i,
	});

	await captureDocument(document, {
		name: "prompt-from-github-issue-01-detail",
		expectations: [
			'Issue detail for "#42 Fix the login redirect" is open beside the issues list.',
			'A primary "Agent..." button sits beside "Open in Web" in the issue header.',
		],
	});

	await user.click(agentButton);

	const promptDialog = (
		await screen.findByRole("heading", {
			name: "Start a new agent session",
		})
	).closest('[data-testid="modal"]');
	expect(promptDialog).toBeTruthy();

	const chip = await within(promptDialog as HTMLElement).findByTestId(
		"issue-chip",
	);
	expect(chip).toHaveTextContent("GitHub #42");
	// Like every tracker, a GitHub issue runs in its own workspace, so the
	// workspace picker gives way to the workspace the issue will open.
	expect(
		within(promptDialog as HTMLElement).getByText("Opens a workspace for #42"),
	).toBeInTheDocument();

	await captureDocument(document, {
		name: "prompt-from-github-issue-02-dialog-chip",
		expectations: [
			'The agent prompt dialog titled "Start a new agent session" is open over the GitHub panel.',
			'A chip labeled "GitHub #42" with a green issue icon is visible above the "Describe a task..." textarea inside the prompt input.',
			'In place of the branch picker, a muted line reads "Opens a workspace for #42".',
		],
	});
}, 60000);
