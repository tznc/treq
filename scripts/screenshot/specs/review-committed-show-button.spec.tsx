import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("captures Committed section Show button on the Changes tab", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();

	const workspaceId = await createWorkspace(repoPath, "feat/committed-show");
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("Workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(workspacePath, "committed.txt", "committed\n");
	await createCommit(repoPath, workspaceId, "add committed file");
	writeWorkspaceFile(workspacePath, "working.txt", "working\n");

	render(<Dashboard />);
	await clickGpuiRow("feat/committed-show");

	const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
	await user.click(reviewTab);
	await screen.findByRole("tab", { name: /^Changes/, selected: true });

	await screen.findByTitle("committed.txt");
	await screen.findByTitle("working.txt");
	const showButton = await screen.findByRole("button", { name: /^Show$/ });

	await captureDocument(document, {
		name: "review-committed-show-button-01-visible",
		expectations: [
			"The Review sidebar Committed section header has a right-aligned Show button with an eye icon.",
			"There is no Committed toggle button in a toolbar above the diff pane.",
			"committed.txt appears under Committed and working.txt appears under Changes.",
		],
	});

	await user.click(showButton);
	await screen.findByRole("button", { name: /^Show$/ });

	await captureDocument(document, {
		name: "review-committed-show-button-02-hidden",
		expectations: [
			"After clicking Show, the Committed section header remains with Show, but committed.txt is no longer listed.",
			"working.txt remains listed under Changes.",
			"The main diff pane no longer shows the committed.txt hunk.",
		],
	});
}, 60000);
