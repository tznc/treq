/**
 * Verifies kicking off an agent prompt from a Linear issue: "Kick off" on the
 * Kanban card opens the agent prompt dialog with the Linear issue chip
 * pre-attached. Only the Linear HTTP API is stubbed.
 */

import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import type { LinearIssue } from "../../../src/lib/api-linear";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

const ISSUE: LinearIssue = vi.hoisted(() => ({
	id: "issue-id",
	identifier: "TREQ-281",
	title: "Linear integration should CRUD issues",
	description: "Implement the issue workflow",
	state: { name: "Todo", type: "unstarted" },
	labels: [],
	branch_name: "ty/treq-281-linear-integration",
	parent_id: null,
	sub_issue_ids: [],
	url: "https://linear.app/treq/issue/TREQ-281",
}));

vi.mock("../../../src/lib/api-linear", async (importOriginal) => ({
	...(await importOriginal<typeof import("../../../src/lib/api-linear")>()),
	linearListTeams: vi.fn().mockResolvedValue([]),
	linearListIssues: vi.fn().mockResolvedValue([ISSUE]),
	linearGetViewer: vi.fn().mockResolvedValue({ id: "viewer-id", name: "Viewer" }),
}));

it("opens the agent prompt dialog with a Linear issue chip from Kick off", async () => {
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);

	const user = userEvent.setup();
	render(<Dashboard />);

	sendGpuiAction({ kind: "openLinear" });
	await user.click(await screen.findByRole("tab", { name: "Kanban" }));
	const kickoff = await screen.findByRole("button", { name: "Kick off" });

	await captureDocument(document, {
		name: "linear-kickoff-agent-prompt-01-kanban",
		expectations: [
			"The Linear panel shows a Kanban board with a 'Todo' column.",
			"The card 'TREQ-281 Linear integration should CRUD issues' has a 'Kick off' button.",
		],
	});

	await user.click(kickoff);

	const dialog = (
		await screen.findByRole("heading", { name: "Start a new agent session" })
	).closest('[data-testid="modal"]') as HTMLElement;
	expect(dialog).toBeTruthy();
	const chip = await within(dialog).findByTestId("issue-chip");
	expect(chip).toHaveTextContent("TREQ-281");

	await captureDocument(document, {
		name: "linear-kickoff-agent-prompt-02-dialog-chip",
		expectations: [
			"The 'Start a new agent session' dialog is open over the Linear panel.",
			"A chip labeled 'Linear TREQ-281' with a violet issue icon sits above the 'Describe a task...' textarea.",
			"The Plan and Edit buttons are enabled (not dimmed) even though the textarea is empty.",
		],
	});
}, 60000);
