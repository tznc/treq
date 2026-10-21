import * as React from "react";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitWorkspaceFile,
	createTestRepo,
	getOriginUrl,
	notifyWorkspaceChanged,
	openRepo,
	resolveCommitId,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createWorkspace,
	getWorkspaces,
	jjGitFetchBackground,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/target-moved";

/** Land a commit on the remote default branch, as if a PR merged there. */
function mergeOnRemote(
	repoPath: string,
	tempDirPath: string,
	defaultBranch: string,
) {
	const clonePath = path.join(tempDirPath, "remote_clone_merge");
	execFileSync("git", ["clone", getOriginUrl(repoPath), clonePath], {
		stdio: "pipe",
	});
	const git = (args: string[]) =>
		execFileSync("git", args, { cwd: clonePath, stdio: "pipe" });
	git(["config", "user.email", "test@example.com"]);
	git(["config", "user.name", "Test User"]);
	git(["checkout", defaultBranch]);
	fs.writeFileSync(path.join(clonePath, "merged-upstream.txt"), "merged\n");
	git(["add", "merged-upstream.txt"]);
	git(["commit", "-m", "Merged on remote"]);
	git(["push", "origin", defaultBranch]);
	fs.rmSync(clonePath, { recursive: true, force: true });
}

// Scenario: a workspace with one committed change is open on the Changes tab.
// A PR then merges on the remote default branch and a background fetch moves
// the target bookmark before the workspace is rebased. The
// Changes tab must list only the workspace's own commit, not the newly merged
// file as a reverted ("D") Committed change.
it("keeps target-branch merges out of the Committed section", async () => {
	const { repoPath, tempDirPath, defaultBranch } = createTestRepo(true);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	);
	if (!workspace) {
		throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);
	}
	await commitWorkspaceFile(
		repoPath,
		{ id: workspace.id, path: workspace.workspace_path },
		"workspace-change.txt",
		"workspace change\n",
		"Workspace change",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /Changes/ }));
	await screen.findAllByText("workspace-change.txt");

	// The workspace is already open (and rebased on open) when the merge lands
	// and a background fetch moves the target bookmark underneath it.
	const targetBefore = resolveCommitId(repoPath, defaultBranch);
	mergeOnRemote(repoPath, tempDirPath, defaultBranch);
	await jjGitFetchBackground(repoPath);
	await waitFor(
		() => {
			expect(resolveCommitId(repoPath, defaultBranch)).not.toBe(targetBefore);
		},
		{ timeout: 15000 },
	);
	notifyWorkspaceChanged(workspace.id);
	await waitFor(() => {
		expect(screen.queryByText("Refreshing...")).not.toBeInTheDocument();
	});
	await screen.findAllByText("workspace-change.txt");
	expect(screen.queryAllByText("merged-upstream.txt")).toHaveLength(0);

	await captureDocument(document, {
		name: "changes-tab-target-moved-01-committed",
		expectations: [
			"The Committed section in the left file list shows exactly one file, workspace-change.txt.",
			"merged-upstream.txt does not appear anywhere, neither in the file list nor as a deleted diff.",
		],
	});
}, 60000);
