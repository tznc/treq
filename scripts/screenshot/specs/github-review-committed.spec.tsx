/**
 * Regression: GitHub PR review threads must place inline on committed Review-tab
 * hunks. Previously placement only searched uncommitted `allFileHunks`, so
 * comments on committed PR lines were dumped into the "Outdated" banner even
 * though those lines were clearly visible in the diff.
 *
 * GitHub API calls are stubbed; the jj repo / Review tab / committed hunk
 * loading path is real.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitWorkspaceFile,
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import type { GhReviewThread, PrInfo } from "../../../src/lib/api-types";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/github-review-committed";

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
		id: "PRRT_on_committed_line",
		is_resolved: false,
		// GitHub often flips this after a push even when the line still exists.
		is_outdated: true,
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
];

it("places GitHub review threads on committed Review-tab lines", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	mockGetGitRemoteUrl.mockResolvedValue(REMOTE_INFO);
	mockGetCachedPrInfo.mockResolvedValue(PR_INFO);
	mockGhListPrReviewThreads.mockResolvedValue(THREADS);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

	// Commit the file so the Changes tab shows it only via committedFileHunks
	// (no uncommitted working-tree change).
	await commitWorkspaceFile(
		repoPath,
		{ id: workspaceId, path: workspace.workspace_path },
		"example.ts",
		"export const a = 1;\nexport const b = 2;\nexport const c = 3;",
		"add example module",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	// Must render inline on the committed line — not an unplaced/outdated section.
	await screen.findByText("This magic number should be a named constant.");
	expect(
		screen.queryByTestId("github-outdated-threads"),
	).not.toBeInTheDocument();
	expect(
		screen.queryByTestId("github-outdated-group-toggle"),
	).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "github-review-committed-01-inline",
		expectations: [
			'A blue-accented GitHub review comment card is inline on the committed example.ts diff, showing @octocat\'s comment "This magic number should be a named constant."',
			"There is no outdated/unplaced threads section above the hunk — the comment is placed on the visible line.",
		],
	});
}, 60000);
