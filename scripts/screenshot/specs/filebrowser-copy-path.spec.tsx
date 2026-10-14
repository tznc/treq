/**
 * Verifies the FileBrowser filename copy button: clicking the copy icon next
 * to the relative path copies it and swaps the icon to a check.
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
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/filebrowser-copy-path";

it("captures FileBrowser filename copy icon feedback", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, BRANCH_NAME);
	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.id === workspaceId,
	);
	if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);
	writeWorkspaceFile(
		resolveWorkspacePath(repoPath, workspace.workspace_path),
		"copy-me.ts",
		"export const value = 1;\n",
	);

	const user = userEvent.setup();
	const clipboardSpy = vi
		.spyOn(navigator.clipboard, "writeText")
		.mockResolvedValue(undefined);

	render(<Dashboard />);

	await clickGpuiRow(BRANCH_NAME);
	await user.click(await screen.findByRole("button", { name: "copy-me.ts" }));
	const fileBrowser = within(await screen.findByTestId("file-browser"));
	const copyButton = await fileBrowser.findByRole("button", {
		name: "Copy file path",
	});

	await captureDocument(document, {
		name: "filebrowser-copy-path-01-before",
		expectations: [
			"The FileBrowser header shows the relative path copy-me.ts with a clipboard copy icon next to it.",
			"The copy icon is the clipboard glyph, not a check mark.",
		],
	});

	await user.click(copyButton);

	expect(clipboardSpy).toHaveBeenCalledWith("copy-me.ts");
	expect(
		await fileBrowser.findByRole("button", { name: "File path copied" }),
	).toBeTruthy();

	await captureDocument(document, {
		name: "filebrowser-copy-path-02-after",
		expectations: [
			"The FileBrowser header copy control next to copy-me.ts shows a green check icon instead of the clipboard glyph.",
			"The relative path copy-me.ts is still visible in the file header.",
		],
	});
}, 60000);
