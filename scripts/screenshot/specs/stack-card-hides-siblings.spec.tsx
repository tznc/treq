import * as React from "react";
import { expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createWorkspace,
	getWorkspaces,
	setWorkspaceTargetBranch,
} from "../../../src/lib/api";
import { getFullWorkspacePath } from "../../../src/lib/utils";
import { render, screen, within } from "../../../test/test-utils";
import {
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const PARENT = "feat/stack-parent";
const CURRENT = "feat/stack-current";
const SIBLING = "feat/stack-sibling";
const CHILD_A = "feat/stack-child-a";
const CHILD_B = "feat/stack-child-b";

it("captures the stack card without current-level sibling workspaces", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	await createWorkspace(repoPath, PARENT);
	await createWorkspace(repoPath, CURRENT);
	await createWorkspace(repoPath, SIBLING);
	await createWorkspace(repoPath, CHILD_A);
	await createWorkspace(repoPath, CHILD_B);

	const workspaces = await getWorkspaces(repoPath);
	const current = workspaces.find((ws) => ws.branch_name === CURRENT);
	const sibling = workspaces.find((ws) => ws.branch_name === SIBLING);
	const childA = workspaces.find((ws) => ws.branch_name === CHILD_A);
	const childB = workspaces.find((ws) => ws.branch_name === CHILD_B);
	if (!current || !sibling || !childA || !childB) {
		throw new Error("Expected parent/current/sibling/child workspaces");
	}

	await setWorkspaceTargetBranch(
		repoPath,
		getFullWorkspacePath(current),
		current.id,
		PARENT,
	);
	await setWorkspaceTargetBranch(
		repoPath,
		getFullWorkspacePath(sibling),
		sibling.id,
		PARENT,
	);
	await setWorkspaceTargetBranch(
		repoPath,
		getFullWorkspacePath(childA),
		childA.id,
		CURRENT,
	);
	await setWorkspaceTargetBranch(
		repoPath,
		getFullWorkspacePath(childB),
		childB.id,
		CURRENT,
	);

	render(<Dashboard />);
	await clickGpuiRow(CURRENT);

	const panel = await screen.findByTestId("workspace-stack-panel");
	expect(within(panel).getByText(PARENT)).toBeTruthy();
	expect(within(panel).getByText(CURRENT)).toBeTruthy();
	expect(within(panel).getByText(CHILD_A)).toBeTruthy();
	expect(within(panel).getByText(CHILD_B)).toBeTruthy();
	expect(within(panel).queryByText(SIBLING)).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "stack-card-hides-siblings-01-current-view",
		expectations: [
			`The WORKSPACES sidebar lists ${PARENT}, ${CURRENT}, ${SIBLING}, ${CHILD_A}, and ${CHILD_B}.`,
			`The Code tab Stack card lists ${CHILD_A}, ${CHILD_B}, ${CURRENT}, and ${PARENT}, and does not list ${SIBLING}.`,
			`${CURRENT} is highlighted as the current stack item.`,
		],
	});
}, 60000);
