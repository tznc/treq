import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	notifyWorkspaceChanged,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/sync-indicator";

// Scenario: a workspace is created and pushed, so it is level with the remote
// and the header shows no sync indicator. The user then edits a file and
// commits it from the Changes tab. The "↑1" indicator must appear on its own --
// previously the header stayed stale (no indicator at all) until the
// workspace-status query happened to refetch, e.g. by selecting another
// workspace in the sidebar and navigating back.
it("captures the header sync indicator before and after committing from the Changes tab", async () => {
	const { repoPath } = createTestRepo(true); // withRemote: real bare remote, so ahead/behind is meaningful
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	// Create the workspace through the real "Stack" dialog, not the API helper.
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

	await user.click(
		await within(header).findByRole("button", { name: /^push$/i }),
	);
	await screen.findByText("Pushed to remote");
	await waitFor(() => {
		expect(
			screen.queryByRole("button", { name: /^push$/i }),
		).not.toBeInTheDocument();
	});

	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.branch_name === BRANCH_NAME,
	);
	if (!workspace) {
		throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);
	}
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"sync-indicator.txt",
		"a line that puts the branch ahead of the remote\n",
	);

	await user.click(await screen.findByRole("tab", { name: /Changes/ }));
	notifyWorkspaceChanged(workspace.id);
	await screen.findAllByText("sync-indicator.txt");

	await captureDocument(document, {
		name: "header-sync-indicator-01-before-commit",
		expectations: [
			"The Changes tab is active and the diff pane shows sync-indicator.txt with one added line highlighted green.",
			"The header (top bar, showing the feat/sync-indicator branch) has NO sync control at all -- the workspace is level with the remote, so neither a '↑'/'↓' counter nor the circular refresh-arrows icon is drawn; only the green 'Merge...' button and the overflow menu sit at the right edge.",
			"A commit message box with placeholder 'Message' and a 'Commit' button are visible in the left file sidebar.",
		],
	});

	await user.type(
		await screen.findByPlaceholderText("Message"),
		"Add sync indicator fixture line",
	);
	await user.click(screen.getByRole("button", { name: "Commit" }));

	await screen.findByText("Commit created");
	await within(header).findByText("↑1");

	await captureDocument(document, {
		name: "header-sync-indicator-02-after-commit",
		expectations: [
			"The header now shows an '↑1' sync indicator with the circular refresh-arrows icon, just left of the green 'Merge...' button -- it appeared without navigating away from this workspace.",
			"A 'Commit created' toast with a green check icon is visible in the bottom-left corner.",
			"The Changes tab's left sidebar no longer has a 'Changes' section; sync-indicator.txt now sits under 'Committed', tagged 'A'.",
		],
	});

	// Toasts stay up for 5s.
	await waitFor(
		() => {
			expect(screen.queryByText("Commit created")).not.toBeInTheDocument();
		},
		{ timeout: 7000 },
	);

	await captureDocument(document, {
		name: "header-sync-indicator-03-ahead-button-styling",
		expectations: [
			"The '↑1' sync button in the header has a visible outline/border around it (not a plain borderless ghost button), since the workspace has an unpushed commit.",
			"A small, semi-transparent dark amber/yellow dot (pip) sits at the top-right corner of the sync button, overlapping its border, indicating unsynced changes.",
		],
	});
}, 60000);
