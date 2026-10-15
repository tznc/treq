import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import fs from "node:fs";
import path from "node:path";
import {
	createTestRepo,
	newCommitWithParents,
	openRepo,
	resolveChangeId,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	ensureWorkspaceIndexed,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

it("captures deleted file placeholder in the Changes tab", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, "feat/deleted-file-qa");
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("Workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(
		workspacePath,
		"doomed.txt",
		"secret line one\nsecret line two\nsecret line three\n",
	);
	await createCommit(repoPath, workspaceId, "add doomed file");
	fs.unlinkSync(path.join(workspacePath, "doomed.txt"));

	const user = userEvent.setup();
	render(<Dashboard />);
	await clickGpuiRow("feat/deleted-file-qa");
	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });

	await waitFor(() => {
		expect(screen.getAllByText("doomed.txt").length).toBeGreaterThan(0);
	});
	await screen.findByText("Deleted");
	await screen.findByTestId("deleted-file-placeholder");

	await captureDocument(document, {
		name: "review-deleted-file-01-placeholder",
		expectations: [
			'The Review main pane shows a doomed.txt file row with a red "Deleted" badge in the header.',
			'The expanded file body shows a centered "File deleted" placeholder instead of the former file contents.',
			"The former content lines (secret line one/two/three) are not visible in the diff area.",
		],
	});
}, 60000);

it("captures delete/modify conflict deleted-side card in the Changes tab", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();

	const workspaceId = await createWorkspace(
		repoPath,
		"feat/delete-modify-qa",
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
	await createCommit(repoPath, workspaceId, "workspace modify");
	const workspaceChangeId = resolveChangeId(workspacePath, "@-");

	fs.unlinkSync(path.join(repoPath, "README.md"));
	await createCommit(repoPath, null, "main delete");
	const mainChangeId = resolveChangeId(repoPath, "@-");

	newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
	await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

	render(<Dashboard />);
	await waitForGpuiConflict(workspaceId);

	await clickGpuiRow("feat/delete-modify-qa");
	const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
	await user.click(reviewTab);
	await screen.findByRole("tab", { name: /^Changes/, selected: true });

	const conflictsToggle = await screen.findByRole("button", {
		name: "Conflicts",
	});
	const conflictsSection = conflictsToggle.closest("div")?.parentElement;
	if (!conflictsSection) throw new Error("Conflicts section not found");
	await user.click(await within(conflictsSection).findByTitle("README.md"));

	await screen.findByText(/^Conflict 1 of 1$/);
	await screen.findByTestId("conflict-deleted-side");

	await captureDocument(document, {
		name: "review-deleted-file-02-conflict-side",
		expectations: [
			'The Review diff shows a conflict card headed "Conflict 1 of 1" for README.md.',
			"A coloured deleted-side card states that one side deleted this file.",
			"The non-deleted side still shows its conflicting content (workspace side and/or base).",
		],
	});
}, 60000);
