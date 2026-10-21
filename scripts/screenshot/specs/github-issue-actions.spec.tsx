/**
 * Verifies the GitHub issue detail actions: the edit and delete buttons, the
 * Close Issue split button and its close-reason menu, the inline title/body
 * editor, and the delete confirmation dialog.
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

it("captures the issue close options, editor, and delete confirmation", async () => {
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
	await user.click(await screen.findByText("Fix the login redirect"));
	await screen.findByText("Issue #42");
	await screen.findByRole("button", { name: "Edit issue" });

	await captureDocument(document, {
		name: "github-issue-actions-01-header-and-close",
		expectations: [
			'The issue header shows pencil (edit) and trash (delete) icon buttons beside "Agent..." and "Open in Web".',
			'"Close Issue" and a chevron button sit joined as one split button next to "Comment".',
		],
	});

	// The capture harness does not render Base UI popups, so check the
	// close-reason menu through the DOM instead of the picture.
	await user.click(screen.getByRole("button", { name: "Close options" }));
	await screen.findByRole("menuitem", { name: "Close as completed" });
	await screen.findByRole("menuitem", { name: "Close as not planned" });
	await user.keyboard("{Escape}");
	await user.click(screen.getByRole("button", { name: "Edit issue" }));
	const titleInput = await screen.findByRole("textbox", {
		name: "Issue title",
	});
	expect(titleInput).toHaveValue("Fix the login redirect");

	await captureDocument(document, {
		name: "github-issue-actions-02-edit-form",
		expectations: [
			'In place of the issue title, a title input holds "Fix the login redirect" with a description textarea below it holding the issue body.',
			'"Save" and "Cancel" buttons sit under the textarea.',
			"The issue body is not also rendered below the editor.",
		],
	});

	await user.click(screen.getByRole("button", { name: "Cancel" }));
	await user.click(
		await screen.findByRole("button", { name: "Delete issue" }),
	);
	const dialog = await screen.findByRole("dialog");
	within(dialog).getByText("Delete issue #42?");

	await captureDocument(document, {
		name: "github-issue-actions-03-delete-confirm",
		expectations: [
			'A modal titled "Delete issue #42?" is centred over the GitHub panel.',
			"It says the delete is permanent and cannot be undone.",
			'It offers "Cancel" and a red "Delete" button.',
		],
	});
}, 60000);
