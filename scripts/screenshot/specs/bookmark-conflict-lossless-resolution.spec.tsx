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
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/lossless-bookmark-qa";

function commitDirectlyToRemote(repoPath: string, tempDirPath: string) {
	const remotePath = getOriginUrl(repoPath);
	const clonePath = path.join(tempDirPath, "bookmark_conflict_remote_clone");
	fs.rmSync(clonePath, { recursive: true, force: true });
	execFileSync("git", ["clone", remotePath, clonePath]);
	execFileSync("git", ["config", "user.email", "test@example.com"], {
		cwd: clonePath,
	});
	execFileSync("git", ["config", "user.name", "Test User"], {
		cwd: clonePath,
	});
	execFileSync("git", ["checkout", BRANCH_NAME], { cwd: clonePath });
	fs.writeFileSync(path.join(clonePath, "remote-only.txt"), "remote\n");
	execFileSync("git", ["add", "remote-only.txt"], { cwd: clonePath });
	execFileSync("git", ["commit", "-m", "remote-only change"], {
		cwd: clonePath,
	});
	execFileSync("git", ["push", "origin", BRANCH_NAME], { cwd: clonePath });
	fs.rmSync(clonePath, { recursive: true, force: true });
}

it("captures lossless bookmark-conflict resolution", async () => {
	const { repoPath, tempDirPath } = createTestRepo(true);
	openRepo(repoPath);
	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	)!;
	const workspacePath = resolveWorkspacePath(repoPath, workspace.workspace_path);

	writeWorkspaceFile(workspacePath, "base.txt", "base\n");
	await createCommit(repoPath, workspaceId, "shared base");
	await pushWorkspaceToRemote(repoPath, workspaceId);
	writeWorkspaceFile(workspacePath, "local-one.txt", "one\n");
	await createCommit(repoPath, workspaceId, "local one");
	writeWorkspaceFile(workspacePath, "local-two.txt", "two\n");
	await createCommit(repoPath, workspaceId, "local two");
	commitDirectlyToRemote(repoPath, tempDirPath);

	const user = userEvent.setup();
	render(<Dashboard />);
	await clickGpuiRow(BRANCH_NAME);

	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
	// Opening the workspace fetches and auto-resolves the conflicted bookmark
	// in the background; the queries pick that up on their next poll, which
	// tests turn off, so revalidate them while waiting.
	await waitFor(
		async () => {
			await act(async () => {
				await invalidateQueries();
			});
			expect(screen.getByText("local one")).toBeInTheDocument();
			expect(screen.getByText("local two")).toBeInTheDocument();
			expect(screen.getByText("remote-only change")).toBeInTheDocument();
			expect(screen.getByText("↑2")).toBeVisible();
		},
		{ timeout: 20000, interval: 1000 },
	);
	expect(screen.queryByText(/Resolve bookmark conflict/i)).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "bookmark-conflict-lossless-01-rebased-history",
		expectations: [
			'The workspace is ready with an "↑2" sync action after automatic conflict resolution.',
			'The commit history visibly retains both "local one" and "local two" above "remote-only change".',
		],
	});

	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findByTestId("review-change-count", {}, { timeout: 20000 });
	expect(screen.queryByText(/No changes to review/i)).not.toBeInTheDocument();
	await captureDocument(document, {
		name: "bookmark-conflict-lossless-01b-review-after-resolve",
		expectations: [
			"The Changes tab is selected after the conflicted bookmark was auto-resolved.",
			"Review shows committed file changes rather than an empty 'No changes to review' state.",
			"The slash branch name remains visible in the workspace header.",
		],
	});

	await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
	await screen.findByText("local one", {}, { timeout: 20000 });
	await user.click(screen.getByText("↑2").closest("button")!);
	await waitFor(() => expect(screen.queryByText("↑2")).not.toBeInTheDocument(), {
		timeout: 20000,
	});

	await captureDocument(document, {
		name: "bookmark-conflict-lossless-02-synced",
		expectations: [
			"The workspace no longer shows an ahead count after syncing the preserved commits.",
			'The commit history still visibly contains "local one", "local two", and "remote-only change".',
		],
	});
}, 90000);
