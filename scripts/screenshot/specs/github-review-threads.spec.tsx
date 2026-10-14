/**
 * Verifies GitHub PR review comment threads rendered inline in the Review
 * tab: read-only, styled distinctly from local comments (larger avatar +
 * linked author + per-comment GitHub icon), resolved threads collapsed by
 * default (still showing author + a greyed truncated preview), threads that
 * no longer map onto a visible diff line always shown above the hunk (no
 * parent "Outdated" collapsible), and selecting text inside a comment body
 * surfaces a floating "Quote" button that seeds a normal local draft comment.
 *
 * The GitHub side (remote info / PR lookup / review threads) all shell out
 * to a real `gh` CLI the desktop harness can't reach, so those three API
 * calls are stubbed -- same pattern as merge-queue-tab.spec.tsx. Everything
 * else (the real jj repo, the Changes tab, the diff, the comment pipeline) is
 * the real app.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import type { GhReviewThread, PrInfo } from "../../../src/lib/api-types";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/github-review-threads";

const {
	mockGetGitRemoteUrl,
	mockGetCachedPrInfo,
	mockGhListPrReviewThreads,
} = vi.hoisted(() => ({
	mockGetGitRemoteUrl: vi.fn(),
	mockGetCachedPrInfo: vi.fn(),
	mockGhListPrReviewThreads: vi.fn(),
}));

vi.mock("../../../src/lib/api", async () => {
	const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
		"../../../src/lib/api",
	);
	return {
		...actual,
		getGitRemoteUrl: mockGetGitRemoteUrl,
		getCachedPrInfo: mockGetCachedPrInfo,
		startPrStatusPolling: vi.fn(async () => undefined),
		refreshPrBranchStatus: vi.fn(async () => undefined),
		stopPrStatusPolling: vi.fn(async () => undefined),
		refreshPrStatuses: vi.fn(async () => undefined),
		ghListPrReviewThreads: mockGhListPrReviewThreads,
	};
});

const REMOTE_INFO = {
	owner: "treq-dev",
	repo: "treq",
	full_name: "treq-dev/treq",
};

const PR_INFO: PrInfo = {
	number: 1,
	title: "Add example module",
	state: "OPEN",
	url: "https://github.com/treq-dev/treq/pull/1",
	head_ref_name: BRANCH_NAME,
	base_ref_name: "main",
	merge_state_status: "CLEAN",
	is_draft: false,
};

const THREADS: GhReviewThread[] = [
	{
		id: "PRRT_unresolved",
		is_resolved: false,
		is_outdated: false,
		path: "example.ts",
		line: 2,
		diff_side: "RIGHT",
		comments: [
			{
				id: "PRRC_1",
				body: "This magic number should be a named constant.",
				author: { login: "octocat", avatar_url: null },
				created_at: "2026-07-20T10:00:00Z",
				diff_hunk:
					"@@ -0,0 +1,3 @@\n+export const a = 1;\n+export const b = 2;",
				url: "https://github.com/treq-dev/treq/pull/1#discussion_r1",
			},
		],
	},
	{
		id: "PRRT_resolved",
		is_resolved: true,
		is_outdated: false,
		path: "example.ts",
		line: 1,
		diff_side: "RIGHT",
		comments: [
			{
				id: "PRRC_2",
				body: "Looks good now, thanks!",
				author: { login: "reviewer-bot", avatar_url: null },
				created_at: "2026-07-20T11:00:00Z",
				diff_hunk: "@@ -0,0 +1,3 @@\n+export const a = 1;",
				url: "https://github.com/treq-dev/treq/pull/1#discussion_r2",
			},
		],
	},
	{
		id: "PRRT_outdated",
		is_resolved: true,
		is_outdated: true,
		path: "example.ts",
		line: null,
		diff_side: "RIGHT",
		comments: [
			{
				id: "PRRC_3",
				body: "This comment is on a line that no longer exists in the diff.",
				author: { login: "old-reviewer", avatar_url: null },
				created_at: "2026-07-19T09:00:00Z",
				diff_hunk: "@@ -1 +1 @@\n-old line",
				url: "https://github.com/treq-dev/treq/pull/1#discussion_r3",
			},
		],
	},
];

it("captures GitHub review comment threads in the Changes tab", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	mockGetGitRemoteUrl.mockResolvedValue(REMOTE_INFO);
	mockGetCachedPrInfo.mockResolvedValue(PR_INFO);
	mockGhListPrReviewThreads.mockResolvedValue(THREADS);

	// Incidental background state: the workspace/file aren't what's under
	// test, the GitHub thread rendering on top of them is.
	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"example.ts",
		"export const a = 1;\nexport const b = 2;\nexport const c = 3;\n",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	// Unresolved thread renders inline, expanded, with its comment visible.
	await screen.findByText("This magic number should be a named constant.");
	// Resolved thread starts collapsed but still shows author + greyed preview.
	await screen.findByRole("link", { name: "@reviewer-bot" });
	expect(
		await screen.findAllByTestId("github-thread-collapsed-preview"),
	).not.toHaveLength(0);
	expect(screen.queryByTestId("github-outdated-group-toggle")).not.toBeInTheDocument();
	// Outdated/unplaced threads are always visible (no parent collapsible).
	expect(
		await screen.findAllByTestId("github-outdated-threads"),
	).not.toHaveLength(0);
	await screen.findByRole("link", { name: "@old-reviewer" });
	expect(
		screen.getAllByText(
			"This comment is on a line that no longer exists in the diff.",
		).length,
	).toBeGreaterThan(0);

	await captureDocument(document, {
		name: "github-review-threads-01-inline",
		expectations: [
			'A blue-accented card is inline in the example.ts diff, expanded to show @octocat\'s comment "This magic number should be a named constant." with a larger avatar, a linked username, a sans-serif comment body, a small GitHub icon on the comment itself (not on the thread header), and a date without a year.',
			'A second, collapsed GitHub thread card shows a small avatar, linked @reviewer-bot username, a gray "Resolved" badge, then a greyed truncated preview of "Looks good now, thanks!" on the same row after Resolved.',
			'Above the diff hunk, an outdated/unplaced GitHub thread card is visible without any parent "N outdated comments" collapsible banner; it shows @old-reviewer, Resolved, and a greyed preview of the outdated comment body on one row.',
		],
	});

	// Each comment also exposes a dedicated "Quote this comment" button at its
	// right edge (revealed on hover -- a CSS-only affordance the static-HTML
	// screenshot harness can't rasterize as hovered, so this only asserts it
	// exists and quotes the whole comment body when clicked).
	const commentBody = (
		await screen.findAllByTestId("github-comment-body")
	).find((el) => el.textContent?.includes("magic number"));
	if (!commentBody) throw new Error("comment body not found");
	const quoteWholeCommentBtn = await screen.findByTitle("Quote this comment");
	await user.click(quoteWholeCommentBtn);
	await screen.findByText(/quoting @octocat/i);
	// Cancel that draft -- the rest of the spec drives the selection-based
	// quote flow instead, to also cover partial-excerpt quoting.
	await user.click(await screen.findByRole("button", { name: /^cancel$/i }));

	// Select text inside the unresolved comment's body and quote it.
	// (Range.getBoundingClientRect is polyfilled globally in test/setup.common.ts --
	// jsdom doesn't implement it, but real browsers do.)
	const textNode = commentBody.firstChild;
	if (!textNode) throw new Error("comment body has no text node");
	const range = document.createRange();
	range.selectNodeContents(textNode);
	const selection = window.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
	document.dispatchEvent(new Event("selectionchange"));

	await user.click(await screen.findByRole("button", { name: /quote/i }));
	await captureDocument(document, {
		name: "github-review-threads-02-quote-toolbar",
		expectations: [
			'A comment composer with placeholder "Add a comment..." is now open right under the GitHub thread, on the example.ts:2 line.',
			"The quoted GitHub comment text still shows above the composer as the code/quote block being replied to.",
		],
	});

	await user.type(
		await screen.findByPlaceholderText(/add a comment/i),
		"Agreed, will extract a constant.",
	);
	await waitFor(async () => {
		const btns = screen.getAllByRole("button", { name: /add comment/i });
		const submitBtn = btns.find(
			(btn) => btn.textContent?.trim() === "Add Comment",
		);
		expect(submitBtn).toBeDefined();
		await user.click(submitBtn!);
	});
	await screen.findByText("Agreed, will extract a constant.");

	await captureDocument(document, {
		name: "github-review-threads-03-quoted-comment-added",
		expectations: [
			'A new local, editable comment card reading "Agreed, will extract a constant." is now visible on the example.ts diff, distinct from the read-only GitHub thread card above it.',
			'The "Finish review" action bar at the top now shows a pending comment count of at least 1.',
		],
	});
}, 60000);
