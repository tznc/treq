import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces, pushWorkspaceToRemote } from "../../../src/lib/api";
import { invalidateQueries } from "../../../src/lib/swr-cache";
import type { PrCiStatus } from "../../../src/lib/api-types";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
	commitRepoFile,
	commitWorkspaceFile,
	createTestRepo,
	openRepo,
} from "../../../test/utils";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/merge-button-demo";

// Merge queue is irrelevant to this flow and would otherwise need Supabase
// stubbed too -- turn it off so the header only shows the Merge... button
// (and its CI-driven color) this spec cares about.
vi.mock("../../../src/lib/features", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("../../../src/lib/features")>();
	return {
		...actual,
		FEATURES: {
			...actual.FEATURES,
			pro: true,
			stripePayments: false,
			emailSignup: false,
			mergeQueue: false,
		},
	};
});

const { mockGetCachedPrInfo, mockGetCachedPrCiStatus, mockGetGitRemoteUrl } =
	vi.hoisted(() => ({
		mockGetCachedPrInfo: vi.fn(),
		mockGetCachedPrCiStatus: vi.fn(),
		mockGetGitRemoteUrl: vi.fn(),
	}));

vi.mock("../../../src/lib/api", async () => {
	const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
		"../../../src/lib/api",
	);
	return {
		...actual,
		getCachedPrInfo: mockGetCachedPrInfo,
		startPrStatusPolling: vi.fn(async () => undefined),
		stopPrStatusPolling: vi.fn(async () => undefined),
		refreshPrStatuses: vi.fn(async () => undefined),
		refreshPrBranchStatus: vi.fn(async () => undefined),
		getCachedPrCiStatus: mockGetCachedPrCiStatus,
		// createTestRepo's remote is a local bare repo, so the real
		// get_git_remote_url returns null and the PR/CI hooks short-circuit.
		// Stub it to the GitHub remote a real user would have.
		getGitRemoteUrl: mockGetGitRemoteUrl,
	};
});

const REMOTE_INFO = {
	owner: "treq-dev",
	repo: "treq",
	full_name: "treq-dev/treq",
};

function ciStatus(overrides: Partial<PrCiStatus> = {}): PrCiStatus {
	return {
		state: "success",
		total: 2,
		passed: 2,
		failed: 0,
		pending: 0,
		checks: [
			{ name: "build", bucket: "pass", link: "https://github.com/x/1" },
			{ name: "lint", bucket: "pass", link: "https://github.com/x/2" },
		],
		...overrides,
	};
}

function stubPr() {
	mockGetCachedPrInfo.mockResolvedValue({
		number: 42,
		title: "Merge button demo",
		state: "OPEN",
		url: "https://github.com/treq-dev/treq/pull/42",
		head_ref_name: BRANCH_NAME,
		base_ref_name: "main",
		merge_state_status: "CLEAN",
	});
	mockGetGitRemoteUrl.mockResolvedValue(REMOTE_INFO);
}

// Real repo + workspace + push, driven through the app's own affordances --
// the CI status indicator (and the Merge button's CI-driven color) is only
// wired up once the branch exists on remote.
async function setupPushedWorkspace(user: ReturnType<typeof userEvent.setup>) {
	const { repoPath } = createTestRepo(true);
	openRepo(repoPath);
	await commitRepoFile(repoPath, "base.txt", "base", "Base commit");

	render(<Dashboard />);
	await screen.findByTestId("show-workspace-header");
	await user.click(await screen.findByRole("button", { name: "Stack" }));

	const dialog = await screen.findByTestId("modal");
	await user.type(within(dialog).getByLabelText("Branch Name"), BRANCH_NAME);
	await user.click(
		within(dialog).getByRole("button", { name: "Create Workspace" }),
	);
	await waitFor(() => {
		expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
	});

	const workspace = (await getWorkspaces(repoPath)).find(
		(candidate) => candidate.branch_name === BRANCH_NAME,
	);
	if (!workspace) throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);

	await commitWorkspaceFile(
		repoPath,
		{ id: workspace.id, path: workspace.workspace_path },
		"demo-file.txt",
		"demo content",
		"Workspace commit for merge button demo",
	);

	await pushWorkspaceToRemote(repoPath, workspace.id);
	await invalidateQueries();
	await waitFor(async () => {
		const pushed = (await getWorkspaces(repoPath)).find(
			(candidate) => candidate.branch_name === BRANCH_NAME,
		);
		expect(pushed?.not_on_remote).toBe(false);
	});
}

it("captures the outline-green Merge button when CI is not all passing", async () => {
	const user = userEvent.setup();
	stubPr();
	mockGetCachedPrCiStatus.mockResolvedValue(
		ciStatus({
			state: "pending",
			passed: 1,
			pending: 1,
			checks: [
				{ name: "build", bucket: "pass", link: "https://github.com/x/1" },
				{ name: "test", bucket: "pending", link: "https://github.com/x/2" },
			],
		}),
	);

	await setupPushedWorkspace(user);

	await screen.findByRole("button", { name: "Checks: GitHub 1/2" });
	await captureDocument(document, {
		name: "merge-button-color-states-01-outline",
		expectations: [
			'The "Merge..." button in the workspace header has a plain neutral outline (a gray/input-colored border, no green anywhere on it) with normal foreground text/icon color, and its interior background matches the surrounding card background.',
			'The header "Checks" button shows yellow "GH 1/2" text to the left of the Merge button, showing CI has not finished passing.',
		],
	});
}, 120000);

it("captures the solid-green Merge button when CI is fully passing", async () => {
	const user = userEvent.setup();
	stubPr();
	mockGetCachedPrCiStatus.mockResolvedValue(ciStatus());

	await setupPushedWorkspace(user);

	await screen.findByRole("button", { name: "Checks: GitHub 2/2" });
	await captureDocument(document, {
		name: "merge-button-color-states-02-solid",
		expectations: [
			'The "Merge..." button in the workspace header is filled with a solid green background and white text, clearly distinct from an outline style.',
			'The header "Checks" button shows a green check icon and green "GH 2/2" text to the left of the Merge button.',
		],
	});
}, 120000);
