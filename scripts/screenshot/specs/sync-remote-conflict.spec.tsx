import * as React from "react";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	getOriginUrl,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { act, render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	getWorkspaces,
	pushWorkspaceToRemote,
} from "../../../src/lib/api";
import { invalidateQueries } from "../../../src/lib/swr-cache";
import { captureDocument } from "../capture";
import { clickGpuiRow, gpuiRows } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/sync-remote-conflict";

/** Commit on a branch in the bare remote without touching the local repo. */
function remoteCommitOnBranch(
	repoPath: string,
	tempDirPath: string,
	branchName: string,
	relativePath: string,
	content: string,
	message: string,
) {
	const remotePath = getOriginUrl(repoPath);
	const clonePath = path.join(tempDirPath, "remote_clone_branch");
	if (fs.existsSync(clonePath)) {
		fs.rmSync(clonePath, { recursive: true, force: true });
	}

	execFileSync("git", ["clone", remotePath, clonePath], { stdio: "pipe" });
	execFileSync("git", ["config", "user.email", "test@example.com"], {
		cwd: clonePath,
		stdio: "pipe",
	});
	execFileSync("git", ["config", "user.name", "Test User"], {
		cwd: clonePath,
		stdio: "pipe",
	});

	try {
		execFileSync("git", ["checkout", branchName], {
			cwd: clonePath,
			stdio: "pipe",
		});
	} catch {
		execFileSync(
			"git",
			["checkout", "-b", branchName, `origin/${branchName}`],
			{ cwd: clonePath, stdio: "pipe" },
		);
	}

	const filePath = path.join(clonePath, relativePath);
	fs.mkdirSync(path.dirname(filePath), { recursive: true });
	fs.writeFileSync(filePath, content);
	execFileSync("git", ["add", relativePath], { cwd: clonePath, stdio: "pipe" });
	execFileSync("git", ["commit", "-m", message], {
		cwd: clonePath,
		stdio: "pipe",
	});
	execFileSync("git", ["push", "origin", branchName], {
		cwd: clonePath,
		stdio: "pipe",
	});
	fs.rmSync(clonePath, { recursive: true, force: true });
}

// Divergent same-file remote edits must surface conflicts in ShowWorkspace, and
// Sync must still complete (pull+push) while those conflicts remain — not block.
it("captures Sync completing while remote conflicts remain for local resolve", async () => {
	const { repoPath, tempDirPath } = createTestRepo(true);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	);
	if (!workspace) {
		throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);
	}
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(workspacePath, "shared.txt", "base\n");
	await createCommit(repoPath, workspaceId, "shared base");
	await pushWorkspaceToRemote(repoPath, workspaceId);

	writeWorkspaceFile(workspacePath, "shared.txt", "local edit\n");
	await createCommit(repoPath, workspaceId, "local edit");

	remoteCommitOnBranch(
		repoPath,
		tempDirPath,
		BRANCH_NAME,
		"shared.txt",
		"remote edit\n",
		"remote edit",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");

	// Opening the workspace fetches/auto-rebases divergent remote tips, which
	// materializes same-file conflicts onto the bookmark for local resolve.
	// The status queries pick that up on their next poll, which tests turn
	// off, so revalidate them while waiting.
	await waitFor(
		async () => {
			await act(async () => {
				await invalidateQueries(["workspace-status"]);
				await invalidateQueries(["workspace-statuses"]);
			});
			expect(screen.getByRole("alert")).toHaveTextContent(
				/1 conflict detected/i,
			);
			expect(gpuiRows().find((r) => r.id === workspaceId)?.conflicted).toBe(
				true,
			);
		},
		{ timeout: 20000, interval: 1000 },
	);

	// Sync control remains available (ahead of remote after bookmark advance).
	await waitFor(
		() => {
			expect(screen.getByText("↑1")).toBeInTheDocument();
		},
		{ timeout: 20000 },
	);

	await captureDocument(document, {
		name: "sync-remote-conflict-01-before-sync",
		expectations: [
			'A red "1 conflict detected" alert is visible in the Code tab.',
			"The Sync control shows ↑1 and is available despite the conflict alert.",
		],
	});

	await user.click(screen.getByText("↑1").closest("button")!);

	await waitFor(
		() => {
			expect(
				screen.getByText(/Synced with conflicts|Synced with remote/i),
			).toBeInTheDocument();
		},
		{ timeout: 20000 },
	);

	// Conflicts remain after Sync — Sync must not clear or hide them.
	expect(screen.getByRole("alert")).toHaveTextContent(/1 conflict detected/i);

	await captureDocument(document, {
		name: "sync-remote-conflict-02-after-sync",
		expectations: [
			"A toast confirms Sync completed (synced with conflicts or synced with remote).",
			'The "1 conflict detected" alert is still visible after Sync.',
		],
	});

	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await screen.findByRole("button", { name: "Conflicts" });

	await captureDocument(document, {
		name: "sync-remote-conflict-03-review-conflicts",
		expectations: [
			'The Changes tab shows a "Conflicts" section listing shared.txt.',
			"The conflicted file remains available for local resolution after Sync.",
		],
	});
}, 90000);
