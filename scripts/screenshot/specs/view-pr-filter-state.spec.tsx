import * as React from "react";
import fs from "node:fs";
import path from "node:path";
import { it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { Dashboard } from "../../../src/components/Dashboard";
import {
	createWorkspace,
	getWorkspaces,
	getCachedPrInfo,
	ghListPrs,
	ghViewPr,
	pushWorkspaceToRemote,
	updateWorkspace,
} from "../../../src/lib/api";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

// gh CLI calls can't run against a real GitHub remote inside the harness, so
// mock the gh-network boundary the same way test/integration/workspace/create-pr.test.tsx
// does. getGitRemoteUrl stays real -- it only reads .git/config off disk.
vi.mock("../../../src/lib/api", async (importOriginal) => {
	const original =
		await importOriginal<typeof import("../../../src/lib/api")>();
	return {
		...original,
		getCachedPrInfo: vi.fn().mockResolvedValue(null),
		startPrStatusPolling: vi.fn(async () => undefined),
		refreshPrBranchStatus: vi.fn(async () => undefined),
		stopPrStatusPolling: vi.fn(async () => undefined),
		refreshPrStatuses: vi.fn(async () => undefined),
		ghViewPr: vi.fn(),
		ghListPrs: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
		ghListIssues: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
	};
});

function setOriginUrl(repoPath: string, remoteUrl: string) {
	const configPath = path.join(repoPath, ".git", "config");
	let config = fs.readFileSync(configPath, "utf-8");
	if (/\[remote "origin"\][\s\S]*?url\s*=/.test(config)) {
		config = config.replace(
			/(\[remote "origin"\][\s\S]*?url\s*=\s*).*/m,
			`$1${remoteUrl}`,
		);
	} else {
		config += `\n[remote "origin"]\n\turl = ${remoteUrl}\n`;
	}
	fs.writeFileSync(configPath, config);
}

it("captures View PR landing on the filter tab matching a CLOSED PR's state", async () => {
	const { repoPath } = createTestRepo(true);
	openRepo(repoPath);

	const workspaceId = await createWorkspace(repoPath, "feat/closed-pr");
	await updateWorkspace(
		repoPath,
		workspaceId,
		undefined,
		"Ship the feature",
		"Implements the feature end-to-end.",
	);
	await pushWorkspaceToRemote(repoPath, workspaceId);
	setOriginUrl(repoPath, "https://github.com/acme/treq.git");
	const workspace = (await getWorkspaces(repoPath)).find(
		(w) => w.branch_name === "feat/closed-pr",
	);
	if (!workspace) throw new Error("workspace not found");

	vi.mocked(getCachedPrInfo).mockResolvedValue({
		number: 10,
		title: "Closed PR",
		state: "CLOSED",
		url: "https://github.com/acme/treq/pull/10",
		head_ref_name: "feat/closed-pr",
		base_ref_name: "main",
		merge_state_status: null,
		is_draft: false,
	});
	vi.mocked(ghViewPr).mockResolvedValue({
		number: 10,
		title: "Closed PR",
		state: "CLOSED",
		url: "https://github.com/acme/treq/pull/10",
		body: "PR body",
		author: { login: "alice" },
		labels: [],
		head_ref_name: "feat/closed-pr",
		base_ref_name: "main",
		merge_state_status: null,
		created_at: "2026-01-01T00:00:00Z",
		updated_at: "2026-01-01T00:00:00Z",
		comments: null,
	});

	const user = userEvent.setup();
	render(<Dashboard />);

	await clickGpuiRow("feat/closed-pr");
	const header = await screen.findByTestId("show-workspace-header");
	const viewPr = await within(header).findByRole("button", {
		name: /view pr.*closed/i,
	});
	await captureDocument(document, {
		name: "view-pr-filter-state-01-workspace-before-click",
		expectations: [
			'A red-styled "View PR" button is visible in the workspace header, indicating a closed PR.',
		],
	});

	await user.click(viewPr);

	await screen.findByText("Closed PR");
	const filterOpen = screen.getByRole("button", { name: "Open" });
	const filterClosed = screen.getByRole("button", { name: "Closed" });
	const filterAll = screen.getByRole("button", { name: "All" });
	if (!filterClosed.className.includes("bg-background")) {
		throw new Error(
			`expected "Closed" filter to be active, got className: ${filterClosed.className}`,
		);
	}
	if (
		filterOpen.className.includes("bg-background") ||
		filterAll.className.includes("bg-background")
	) {
		throw new Error("expected only the \"Closed\" filter to be active");
	}
	await captureDocument(document, {
		name: "view-pr-filter-state-02-closed-filter-active",
		expectations: [
			'The GitHub panel\'s Pull Requests tab is active with the "Closed" filter button visually highlighted (not "Open" or "All").',
			'The PR detail panel on the right shows "Closed PR" (PR #10).',
		],
	});

	if (vi.mocked(ghListPrs).mock.calls.length === 0) {
		throw new Error("expected ghListPrs to have been called");
	}
	const lastCall =
		vi.mocked(ghListPrs).mock.calls[vi.mocked(ghListPrs).mock.calls.length - 1];
	if (lastCall[1] !== "closed") {
		throw new Error(`expected ghListPrs to be called with "closed", got "${lastCall[1]}"`);
	}
}, 60000);
