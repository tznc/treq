import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import {
	createTestRepo,
	newCommitWithParents,
	openRepo,
	resolveChangeId,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { act, render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	ensureWorkspaceIndexed,
	getWorkspaceStatus,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

const BRANCH_NAME = "feat/wc-resolve-clears-conflicts";

type FilesChangedHandler = (event: {
	payload: { workspace_id: number; changed_paths: string[] };
}) => void;

it("clears Conflicts UI when markers are resolved in the working copy", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	);
	if (!workspace) throw new Error("Workspace not found");
	const workspacePath = resolveWorkspacePath(repoPath, workspace.workspace_path);

	writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
	await createCommit(repoPath, workspaceId, "workspace conflicting change");
	const workspaceChangeId = resolveChangeId(workspacePath, "@-");

	writeWorkspaceFile(repoPath, "README.md", "main side\n");
	await createCommit(repoPath, null, "main conflicting change");
	const mainChangeId = resolveChangeId(repoPath, "@-");

	newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
	await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

	const filesChangedHandlers: FilesChangedHandler[] = [];
	vi.mocked(listen).mockImplementation(((event: string, handler: unknown) => {
		if (event === "workspace-files-changed") {
			filesChangedHandlers.push(handler as FilesChangedHandler);
		}
		return Promise.resolve(() => {});
	}) as typeof listen);

	const user = userEvent.setup();
	render(<Dashboard />);
	await waitForGpuiConflict(workspaceId);
	await clickGpuiRow(BRANCH_NAME);
	await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
	await screen.findByRole("tab", { name: /^Changes/, selected: true });
	await screen.findByRole("button", { name: "Conflicts" });
	await screen.findByRole("button", { name: /Resolve conflicts/i });
	expect(screen.getByTestId("review-change-count").className).toMatch(
		/destructive/,
	);

	await captureDocument(document, {
		name: "review-conflict-clears-on-wc-resolve-01-before",
		expectations: [
			'The Changes tab shows a red "Conflicts" section listing README.md.',
			'A red "Resolve conflicts" action bar is visible above the commit area.',
			"The Changes tab badge is red (conflict tone).",
		],
	});

	writeWorkspaceFile(workspacePath, "README.md", "resolved content\n");
	await waitFor(async () => {
		const status = await getWorkspaceStatus(repoPath, workspaceId);
		expect(status.has_conflicts).toBe(false);
		expect(status.conflicted_files).toEqual([]);
	});

	expect(filesChangedHandlers.length).toBeGreaterThan(0);
	await act(async () => {
		for (const handler of filesChangedHandlers) {
			handler({
				payload: {
					workspace_id: workspaceId,
					changed_paths: ["README.md"],
				},
			});
		}
	});

	await waitFor(
		() => {
			expect(
				screen.queryByRole("button", { name: "Conflicts" }),
			).not.toBeInTheDocument();
		},
		{ timeout: 5000 },
	);
	expect(
		screen.queryByRole("button", { name: /Resolve conflicts/i }),
	).not.toBeInTheDocument();
	await waitFor(() => {
		const pill = screen.getByTestId("review-change-count");
		expect(pill.className).toMatch(/yellow/);
		expect(pill.className).not.toMatch(/destructive/);
	});
	expect(screen.getByRole("button", { name: /^Changes/ })).toBeTruthy();

	await captureDocument(document, {
		name: "review-conflict-clears-on-wc-resolve-02-after",
		expectations: [
			'The red "Conflicts" section is gone; README.md appears under Changes.',
			'The "Resolve conflicts" action bar is no longer shown.',
			"The Changes tab badge is yellow (uncommitted changes).",
		],
	});
}, 90000);
