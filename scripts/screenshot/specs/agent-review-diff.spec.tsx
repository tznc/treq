import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitWorkspaceFile,
	createTestRepo,
	openRepo,
	seedAgentReviewComment,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/agent-review-diff";

it("shows the Start Review action and a local agent review comment on a diff", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

	await commitWorkspaceFile(
		repoPath,
		{ id: workspaceId, path: workspace.workspace_path },
		"example.ts",
		"export function add(a: number, b: number) {\n  return a + b;\n}\n",
		"add example module",
	);

	// Seeded before the diff viewer mounts: the review-comments hook disables
	// SWR polling in test mode (see `pollMs`), so only the initial fetch on
	// mount will ever pick up a row inserted this way.
	seedAgentReviewComment(repoPath, {
		workspaceId,
		filePath: "example.ts",
		startLine: 2,
		side: "new",
		commentText: "This looks like it should be subtraction, not addition.",
		suggestedReplacement: "  return a - b;",
	});

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	await waitFor(
		() => {
			expect(
				document.querySelectorAll('[data-testid="agent-review-comment-card"]')
					.length,
			).toBe(1);
		},
		{ timeout: 10000, interval: 250 },
	);
	await screen.findByTestId("agent-review-suggestion");

	await captureDocument(document, {
		name: "agent-review-diff-01-comment-card",
		expectations: [
			'A "Start Review" button and a "1 local review comment" count are visible in the Changes tab action bar.',
			'A review comment card is anchored on line 2 of example.ts, badged "Local" to distinguish it from a GitHub comment.',
			"The suggested change renders as a +/- diff against the original line, with the changed words highlighted.",
			'The card shows Apply, "Send to agent", Resolve, and Delete actions.',
		],
	});
}, 60000);
