/**
 * Verifies the Changes tab file-row (collapsible) copy button: clicking the
 * copy icon next to the file path copies the path and shows a success toast.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	createTestRepo,
	openRepo,
	resolveWorkspacePath,
	writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/copy-file-path";

it("captures the Changes tab collapsible copy file path button", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"example.ts",
		"export const a = 1;\nexport const b = 2;\n",
	);

	const user = userEvent.setup();
	const clipboardSpy = vi
		.spyOn(navigator.clipboard, "writeText")
		.mockResolvedValue(undefined);

	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("tab", { name: /^Changes/i }));
	await screen.findAllByText("example.ts");

	const copyButton = await screen.findByRole("button", {
		name: "Copy file path",
	});

	await captureDocument(document, {
		name: "review-copy-file-path-01-before",
		expectations: [
			"The Changes tab is active with the example.ts file row header visible.",
			"A copy (clipboard) icon sits next to the example.ts path in the collapsible header.",
			"No 'Copied' success toast is visible yet.",
		],
	});

	await user.click(copyButton);

	await waitFor(() => {
		expect(clipboardSpy).toHaveBeenCalledWith("example.ts");
	});
	expect(
		await screen.findByText("File path copied to clipboard"),
	).toBeInTheDocument();

	await captureDocument(document, {
		name: "review-copy-file-path-02-after",
		expectations: [
			"A success toast titled 'Copied' is visible after clicking the copy icon.",
			"The toast description reads 'File path copied to clipboard'.",
			"The example.ts file row header and copy icon remain visible in the Changes tab.",
		],
	});
}, 60000);
