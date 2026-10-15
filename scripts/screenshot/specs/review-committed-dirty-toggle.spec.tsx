import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createCommit,
	createWorkspace,
	getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

it("keeps dirty committed files visible after hiding Committed Show", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();

	const workspaceId = await createWorkspace(
		repoPath,
		"feat/committed-dirty-show",
	);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error("Workspace not found");
	const workspacePath = resolveWorkspacePath(
		repoPath,
		workspace.workspace_path,
	);

	writeWorkspaceFile(workspacePath, "shared.txt", "shared v1\n");
	await createCommit(repoPath, workspaceId, "commit shared file");
	writeWorkspaceFile(workspacePath, "shared.txt", "shared v2\n");
	writeWorkspaceFile(workspacePath, "committed-only.txt", "committed only\n");
	await createCommit(repoPath, workspaceId, "commit committed-only file");
	writeWorkspaceFile(workspacePath, "shared.txt", "shared v3\n");
	writeWorkspaceFile(workspacePath, "uncommitted-only.txt", "working only\n");

	render(<Dashboard />);
	await clickGpuiRow("feat/committed-dirty-show");

	const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
	await user.click(reviewTab);
	await screen.findByRole("tab", { name: /^Changes/, selected: true });

	await waitFor(() => {
		expect(screen.getByTitle("shared.txt")).toBeInTheDocument();
		expect(screen.getByTitle("committed-only.txt")).toBeInTheDocument();
		expect(screen.getByTitle("uncommitted-only.txt")).toBeInTheDocument();
	});

	await captureDocument(document, {
		name: "review-committed-dirty-toggle-01-show-on",
		expectations: [
			"shared.txt and uncommitted-only.txt are listed under Changes.",
			"committed-only.txt is listed under Committed with Show on (eye icon).",
			"The Review diff pane shows hunks for the dirty and committed files.",
		],
	});

	await user.click(await screen.findByRole("button", { name: /^Show$/ }));

	await waitFor(() => {
		expect(screen.queryByTitle("committed-only.txt")).not.toBeInTheDocument();
		expect(screen.getByTitle("shared.txt")).toBeInTheDocument();
		expect(screen.getByTitle("uncommitted-only.txt")).toBeInTheDocument();
	});

	await captureDocument(document, {
		name: "review-committed-dirty-toggle-02-show-off",
		expectations: [
			"After hiding Committed, committed-only.txt is gone from the sidebar.",
			"shared.txt remains listed under Changes (dirty overlapping file).",
			"uncommitted-only.txt remains listed under Changes.",
		],
	});
}, 60000);
