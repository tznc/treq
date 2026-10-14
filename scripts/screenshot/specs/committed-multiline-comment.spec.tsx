/**
 * Regression: multi-line drag selection + comment composer must work on
 * committed Review-tab hunks (committedFileHunks), not only uncommitted
 * working-tree diffs.
 */

import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitWorkspaceFile,
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/committed-multiline-comment";

it("selects multiple committed lines and opens a comment composer", async () => {
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
		"export const a = 1;\nexport const b = 2;\nexport const c = 3;\nexport const d = 4;",
		"add example module",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	await waitFor(() => {
		expect(document.querySelectorAll("[data-diff-line]").length).toBeGreaterThan(
			2,
		);
	});

	const diffLines = document.querySelectorAll("[data-diff-line]");
	const line2 = diffLines[1] as HTMLElement;
	const line3 = diffLines[2] as HTMLElement;
	const line4 = diffLines[3] as HTMLElement;
	const startGutter = line2.querySelector(
		"[data-testid='line-gutter']",
	) as HTMLElement;
	expect(startGutter).toBeTruthy();

	await user.pointer([
		{ keys: "[MouseLeft>]", target: startGutter },
		{ target: line3 },
		{ target: line4 },
		{ keys: "[/MouseLeft]" },
	]);

	await waitFor(() => {
		expect(line2.className).toContain("bg-blue-500/10");
		expect(line4.className).toContain("bg-blue-500/10");
	});

	await captureDocument(document, {
		name: "committed-multiline-comment-01-selection",
		expectations: [
			"The Changes tab shows committed example.ts with lines b/c/d highlighted in blue as a multi-line selection.",
			"Line a remains unselected (green add only), confirming the drag covered L2-L4.",
		],
	});

	const commentBtn = line4.querySelector(
		"[data-comment-button]",
	) as HTMLElement;
	expect(commentBtn).toBeTruthy();
	await user.click(commentBtn);

	const composer = await screen.findByPlaceholderText(/add a comment/i);
	await user.type(composer, "Please extract these constants into a config.");

	await captureDocument(document, {
		name: "committed-multiline-comment-02-composer",
		expectations: [
			'A comment composer is open on the committed diff with text "Please extract these constants into a config."',
			"The selected committed lines remain highlighted behind/above the composer.",
		],
	});
}, 60000);
