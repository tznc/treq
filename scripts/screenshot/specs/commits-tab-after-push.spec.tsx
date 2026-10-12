import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitRepoFile,
	commitWorkspaceFile,
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/commits-tab-demo";

// Scenario: 5 commits land on the default branch, the user creates a
// workspace via the home repo's "Stack" button, makes 2 workspace commits,
// then pushes that workspace to the remote. Captures the Commits tab right
// before and right after the push so the "Push" affordance
// disappearing is visible in the diff.
it("captures the Commits tab before and after pushing a workspace to remote", async () => {
	const { repoPath } = createTestRepo(true); // withRemote: real bare remote to push against
	openRepo(repoPath);

	for (let i = 1; i <= 5; i++) {
		await commitRepoFile(
			repoPath,
			`repo-file-${i}.txt`,
			`repo commit content ${i}`,
			`Repo commit ${i}`,
		);
	}

	const user = userEvent.setup();
	render(<Dashboard />);

	// Create the workspace through the real "Stack" button flow (home repo
	// header), not the createWorkspace API helper -- this is the actual
	// affordance a user has for creating a workspace from the home view.
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("button", { name: "Stack" }));

	const dialog = await screen.findByTestId("modal");
	const branchNameInput = within(dialog).getByLabelText("Branch Name");
	await user.type(branchNameInput, BRANCH_NAME);
	await user.click(
		within(dialog).getByRole("button", { name: "Create Workspace" }),
	);
	await waitFor(() => {
		expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
	});

	const header = await screen.findByTestId("show-workspace-header");
	await within(header).findByText(BRANCH_NAME);

	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.branch_name === BRANCH_NAME,
	);
	if (!workspace) {
		throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);
	}

	await commitWorkspaceFile(
		repoPath,
		{ id: workspace.id, path: workspace.workspace_path },
		"workspace-file-1.txt",
		"workspace commit content 1",
		"Workspace commit 1",
	);
	await commitWorkspaceFile(
		repoPath,
		{ id: workspace.id, path: workspace.workspace_path },
		"workspace-file-2.txt",
		"workspace commit content 2",
		"Workspace commit 2",
	);

	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

	await screen.findByText("Workspace commit 2");
	await screen.findByText("Workspace commit 1");
	const pushButton = await screen.findByRole("button", {
		name: /^push$/i,
	});
	await captureDocument(document, {
		name: "commits-tab-after-push-01-before-push",
		expectations: [
			"The Commits tab is active and shows a 'Today' section with Workspace commit 2 above Workspace commit 1.",
			"A 'Recent on <branch>' section below lists 5 repo commits (Repo commit 1..5) plus the Initial commit, each tagged Immutable.",
			"The header shows a blue 'Push' button (workspace not yet pushed).",
		],
	});

	await user.hover(pushButton);
	expect(await screen.findByRole("tooltip")).toHaveTextContent(
		"Push to remote",
	);
	await captureDocument(document, {
		name: "commits-tab-after-push-01b-push-tooltip",
		expectations: [
			"A tooltip reading 'Push to remote' is visible (jsdom has no layout, so it sits at the top-left rather than beside the button).",
			"The header button still reads just 'Push'.",
		],
	});
	await user.unhover(pushButton);

	await user.click(pushButton);
	await screen.findByText("Pushed to remote");

	// Dashboard keeps `selectedWorkspace` synced to the `workspaces` query, so
	// the "Push" button disappears on its own once the post-push
	// refetch lands -- no re-navigation required.
	await waitFor(() => {
		expect(
			screen.queryByRole("button", { name: /^push$/i }),
		).not.toBeInTheDocument();
	});
	// Let the post-push status/toast settle before snapshotting.
	await new Promise((resolve) => setTimeout(resolve, 500));

	await captureDocument(document, {
		name: "commits-tab-after-push-02-after-push",
		expectations: [
			"The 'Push' button is gone from the header (only 'Merge...' and the overflow menu remain).",
			"A toast in the bottom-left corner with a green check icon reads 'Pushed to remote' / 'Changes pushed successfully'.",
			"Workspace commit 1 and Workspace commit 2 are still listed, unchanged from the before-push screenshot.",
		],
	});
}, 60000);
