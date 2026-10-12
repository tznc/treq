import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitRepoFile,
	createTestRepo,
	findSidebarBranchElement,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/sync-after-commit";

// Scenario for the "sync moves local state back a commit" bug: a workspace is
// stacked on a branch that already exists on the remote, the user commits a
// change through the real commit box, then presses the Sync control.
//
// Before the fix, the sync leg moved @ back onto the bookmark: the committed
// change reappeared as an uncommitted current change, the empty working-copy
// commit vanished, and an empty commit was pushed to the remote. The captures
// below are before/after the Sync click so that regression is visible.
it("captures workspace state before and after syncing a fresh commit", async () => {
	const { repoPath, defaultBranch } = createTestRepo(true); // withRemote: real bare remote to sync against
	openRepo(repoPath);

	await commitRepoFile(
		repoPath,
		"repo-file.txt",
		"repo content",
		"Repo baseline commit",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	// Stack a new workspace through the real "Stack" dialog.
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("button", { name: "Stack" }));

	const dialog = await screen.findByTestId("modal");
	await user.type(within(dialog).getByLabelText("Branch Name"), BRANCH_NAME);
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
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	// Background state: publish the branch so it exists on the remote, which is
	// what the Sync control (as opposed to the first-push button) operates on.
	await user.click(
		await screen.findByRole("button", { name: /^push$/i }),
	);
	await screen.findByText("Pushed to remote");
	await waitFor(() => {
		expect(
			screen.queryByRole("button", { name: /^push$/i }),
		).not.toBeInTheDocument();
	});

	// The behavior under test starts here: a real edit, committed via the real
	// commit box, then Sync.
	writeWorkspaceFile(workspacePath, "synced.txt", "synced content\n");

	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await waitFor(() =>
		expect(screen.getAllByText("synced.txt").length).toBeGreaterThan(0),
	);

	await user.type(
		await screen.findByPlaceholderText("Message"),
		"Commit that must survive sync",
	);
	await user.click(await screen.findByRole("button", { name: "Commit" }));

	// After committing, synced.txt has moved out of the "Changes" (current)
	// section and into the "Committed" section.
	await waitFor(() => {
		expect(screen.getAllByText("Committed").length).toBeGreaterThan(0);
		expect(screen.queryByRole("button", { name: /^Changes$/ })).not.toBeInTheDocument();
	});

	// Visit the home repo and come back so the workspace-status query refetches
	// and the header reflects the new ahead-of-remote count.
	await user.click(await findSidebarBranchElement(defaultBranch));
	await user.click(await findSidebarBranchElement(BRANCH_NAME));
	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
	await screen.findByText("Commit that must survive sync");

	// The sync control is icon-only; the '↑1' ahead-count text identifies it.
	// It appears once the workspace-status query reports the workspace is ahead.
	await waitFor(() => expect(screen.getByText("↑1")).toBeInTheDocument(), {
		timeout: 20000,
	});

	await captureDocument(document, {
		name: "sync-after-commit-01-before-sync",
		expectations: [
			"The Commits tab is active and lists 'Commit that must survive sync' as the newest workspace commit.",
			"The header shows the feat/sync-after-commit branch and a sync control reading '↑1' (one commit ahead of remote).",
			"synced.txt is not shown as an uncommitted current change — it is committed.",
		],
	});

	const syncButton = screen.getByText("↑1").closest("button");
	if (!syncButton) {
		throw new Error("Expected the ahead-count to sit inside the sync button");
	}
	await user.click(syncButton);
	await screen.findByText("Synced with remote");
	// Let the post-sync refetch and toast settle before snapshotting.
	await new Promise((resolve) => setTimeout(resolve, 800));

	// First regression assertion: the commit is still a commit.
	await screen.findByText("Commit that must survive sync");

	await captureDocument(document, {
		name: "sync-after-commit-02-after-sync",
		expectations: [
			"'Commit that must survive sync' is still listed in the Commits tab, in the same position as the before screenshot, with no empty '(no description set)' commit above it.",
			"A toast in the bottom-left with a green check icon reads 'Synced with remote' / 'Fetched and pushed changes'.",
			"The '↑1' ahead indicator is gone from the header — the workspace is now in sync with the remote.",
		],
	});

	// Second regression assertion: the committed change did not come back as an
	// uncommitted current change (the headline symptom of the bug).
	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await waitFor(() => {
		expect(screen.getAllByText("Committed").length).toBeGreaterThan(0);
		expect(screen.queryByRole("button", { name: /^Changes$/ })).not.toBeInTheDocument();
	});

	await captureDocument(document, {
		name: "sync-after-commit-03-review-after-sync",
		expectations: [
			"The Changes tab is active and synced.txt appears only under a 'Committed' section.",
			"There is no 'Changes' section — synced.txt has not reverted to being an uncommitted current change.",
		],
	});
}, 90000);
