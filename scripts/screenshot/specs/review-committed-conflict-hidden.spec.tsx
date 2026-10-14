import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	checkAndRebaseWorkspaces,
	createCommit,
	createWorkspace,
	ensureWorkspaceIndexed,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

// Rebase conflicts land in committed hunks. Hiding the Committed section must
// still leave the Conflicts list and inline conflict card visible.
it("captures committed-tip conflict after hiding Committed changes", async () => {
	const { repoPath, defaultBranch } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();

	const workspaceId = await createWorkspace(
		repoPath,
		"feat/committed-conflict-hidden",
	);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("Workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
	await createCommit(repoPath, workspaceId, "workspace conflicting change");

	writeWorkspaceFile(repoPath, "README.md", "main side\n");
	await createCommit(repoPath, null, "main conflicting change");

	await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
	await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

	render(<Dashboard />);
	await waitForGpuiConflict(workspaceId);

	await clickGpuiRow("feat/committed-conflict-hidden");
	const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
	await user.click(reviewTab);
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await screen.findByText("Conflicts");

	await captureDocument(document, {
		name: "review-committed-conflict-hidden-01-before-toggle",
		expectations: [
			'The Review sidebar shows a red "Conflicts" section and a Committed section with a right-aligned Show button.',
			"Committed file rows are visible in the sidebar while Show is active.",
			"The workspace conflict indicator is present for the selected feature branch.",
		],
	});

	const showButton = await screen.findByRole("button", { name: /^Show$/ });
	await user.click(showButton);

	const conflictsToggle = await screen.findByRole("button", {
		name: "Conflicts",
	});
	const conflictsSection = conflictsToggle.closest("div")?.parentElement;
	if (!conflictsSection) throw new Error("Conflicts section not found");
	await user.click(await within(conflictsSection).findByTitle("README.md"));
	await screen.findByText(/^Conflict 1 of 1$/);

	await captureDocument(document, {
		name: "review-committed-conflict-hidden-02-after-hide-committed",
		expectations: [
			"After clicking Show off, the Committed section header remains with Show, but committed file rows are hidden.",
			'The Conflicts section still lists README.md and the diff shows "Conflict 1 of 1".',
			'Conflict side badges "Side #1", "Base", and "Side #2" remain visible on the card.',
		],
	});
}, 60000);
