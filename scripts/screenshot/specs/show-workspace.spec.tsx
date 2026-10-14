import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
	commitRepoFile,
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow, findGpuiRow, stackGpui } from "../../../test/gpui-sidebar";

// Same setup as test/integration/workspace/*.test.tsx: a real jj repo via
// NAPI, a real Dashboard render, real Rust dispatch under the hood. The only
// difference from an integration test is that instead of asserting on the
// DOM, we hand the resulting DOM to a real browser to screenshot it.
it("captures the ShowWorkspace component", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	await createWorkspace(repoPath, "feat/screenshot-demo");
	await commitRepoFile(
		repoPath,
		"NOTES.md",
		"# Notes\n\nSome extra content so the workspace view has more to show.",
		"Add notes",
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await screen.findByTestId("show-workspace-header");
	await findGpuiRow("feat/screenshot-demo");
	await screen.findByRole("heading", { name: "Test Repository" });
	// Let async queries triggered by the initial render (branch list, file
	// tree, README preview, etc.) settle before snapshotting the DOM.
	await new Promise((resolve) => setTimeout(resolve, 500));

	const pngPath = await captureDocument(document, {
		name: "show-workspace",
		expectations: [
			"The header shows the home repo's default branch, with a 'Stack' button next to it.",
			"The sidebar lists one workspace: feat/screenshot-demo.",
			"The Code tab is active and shows the file tree (README.md at least) plus a rendered README preview with the heading 'Test Repository'.",
		],
	});
	console.log(`Saved screenshot -> ${pngPath}`);

	// Switch to the workspace itself, which renders the interactive
	// TargetBranchSelector branch-target visualization (not shown on the
	// home-repo header captured above).
	await clickGpuiRow("feat/screenshot-demo");
	await screen.findByRole("button", { name: "Workspace target" });

	const workspacePngPath = await captureDocument(document, {
		name: "show-workspace-branch-target",
		expectations: [
			"The header shows a branch-target row with the base branch selector and an arrow next to the workspace's own branch name (feat/screenshot-demo).",
		],
	});
	console.log(`Saved screenshot -> ${workspacePngPath}`);

	// Create a stacked workspace with a very long branch name, through the
	// real "Stack" dialog (opened from the workspace's sidebar row), to verify
	// the header truncates it instead of overflowing/pushing out the action
	// buttons.
	const longBranchName =
		"feat/a-very-long-branch-name-that-should-truncate-instead-of-overflowing-the-header-row";
	await stackGpui("feat/screenshot-demo");
	const dialog = await screen.findByTestId("modal");
	const branchNameInput = within(dialog).getByLabelText("Branch Name");
	await user.type(branchNameInput, longBranchName);

	await captureDocument(document, {
		name: "show-workspace-branch-name-input",
		expectations: [
			"The 'Branch Name' input in the Stack dialog is a narrow, fixed-width box that does not grow to fit the long typed branch name -- the text scrolls/overflows within the bounded input rather than the input stretching.",
		],
	});

	await user.click(
		within(dialog).getByRole("button", { name: "Create Workspace" }),
	);
	await waitFor(() => {
		expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
	});

	const longNameHeader = await screen.findByTestId("show-workspace-header");
	await within(longNameHeader).findByText(longBranchName);
	await screen.findByRole("button", { name: "Workspace target" });
	await new Promise((resolve) => setTimeout(resolve, 300));

	const longNamePngPath = await captureDocument(document, {
		name: "show-workspace-branch-target-long-name",
		expectations: [
			"The header's branch name and target-branch selector are both truncated with an ellipsis (…) instead of overflowing past the Stack/Push/Merge buttons on the right.",
		],
	});
	console.log(`Saved screenshot -> ${longNamePngPath}`);

	// Open the target-branch dropdown to verify its trigger and list items
	// also stay within a bounded width for a long branch name.
	await user.click(screen.getByRole("button", { name: "Workspace target" }));
	await screen.findByPlaceholderText("Search branches...");
	await new Promise((resolve) => setTimeout(resolve, 300));

	const dropdownPngPath = await captureDocument(document, {
		name: "show-workspace-branch-target-dropdown",
		expectations: [
			"The open branch-target dropdown list shows branch names truncated with an ellipsis if they're too long to fit the fixed-width popover.",
		],
	});
	console.log(`Saved screenshot -> ${dropdownPngPath}`);
}, 60000);
