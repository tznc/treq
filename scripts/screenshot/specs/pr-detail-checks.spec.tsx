import { expect, it, vi } from "vitest";
import { PrDetailPanel } from "../../../src/components/github-panel/PrDetail";
import type { GhPullRequest, PrCiStatus } from "../../../src/lib/api-types";
import { render, screen } from "../../../test/test-utils";
import { captureDocument } from "../capture";

// PrDetailPanel resolves PR data and CI status entirely through `gh`
// (ghViewPr / getPrChecksForPr), which the desktop harness can't reach in
// this environment. Those two boundaries are stubbed; the panel and its
// "Checks" section -- which shares its data shape, rollup, and styling with
// the workspace header's Checks popover -- are the real components.
const api = vi.hoisted(() => ({
	ghViewPr: vi.fn(),
	getPrChecksForPr: vi.fn(),
}));

vi.mock("../../../src/lib/api", async (importOriginal) => {
	const original = await importOriginal<typeof import("../../../src/lib/api")>();
	return {
		...original,
		ghViewPr: api.ghViewPr,
		getPrChecksForPr: api.getPrChecksForPr,
	};
});

function basePr(overrides: Partial<GhPullRequest> = {}): GhPullRequest {
	return {
		number: 42,
		title: "Add CI status indicator to PR view",
		state: "OPEN",
		url: "https://github.com/acme/treq/pull/42",
		body: "This adds a Checks section to the PR detail view.",
		author: { login: "alice" },
		labels: [],
		head_ref_name: "feat/ci-status",
		base_ref_name: "main",
		merge_state_status: "CLEAN",
		created_at: "2026-01-01T00:00:00Z",
		updated_at: "2026-01-01T00:00:00Z",
		comments: null,
		is_draft: false,
		...overrides,
	};
}

it("captures the Checks section with a mix of CI job statuses", async () => {
	api.ghViewPr.mockReset().mockResolvedValue(basePr());
	api.getPrChecksForPr.mockReset().mockResolvedValue({
		state: "failure",
		total: 3,
		passed: 1,
		failed: 1,
		pending: 1,
		checks: [
			{
				name: "build",
				bucket: "pass",
				link: "https://github.com/acme/treq/actions/runs/1",
				duration_secs: 12,
			},
			{
				name: "test",
				bucket: "fail",
				link: "https://github.com/acme/treq/actions/runs/2",
				duration_secs: 5 * 60 + 56,
			},
			{
				name: "lint",
				bucket: "pending",
				link: "https://github.com/acme/treq/actions/runs/3",
				duration_secs: 45,
			},
		],
	} satisfies PrCiStatus);

	render(
		<PrDetailPanel repoFullName="acme/treq" prNumber={42} onClose={() => {}} />,
	);

	expect(await screen.findByText(/checks \(1\/3\)/i)).toBeVisible();
	const checkRows = screen.getAllByRole("button", {
		name: /^(test|lint|build)/i,
	});
	expect(checkRows.map((button) => button.textContent)).toEqual([
		"test5m 56s",
		"lint45s",
		"build12s",
	]);
	expect(screen.queryByText("Failed")).not.toBeInTheDocument();
	expect(screen.queryByText("Success")).not.toBeInTheDocument();
	expect(screen.queryByText("Pending")).not.toBeInTheDocument();
	// The compact "CI failed" pill (shared with the workspace header) shows
	// next to the state chip in addition to the full per-job list below.
	expect(
		screen.getByRole("button", { name: /CI failed: 1\/3/ }),
	).toBeVisible();

	await captureDocument(document, {
		name: "pr-detail-checks-01-mixed-statuses",
		expectations: [
			'A red-bordered "1/3" pill (matching the workspace header\'s CI status indicator) appears next to the Open state chip, and a "Checks (1/3)" section header appears below it, above the PR description.',
			'Check rows are ordered failed → pending → success: "test" (5m 56s), "lint" (45s), "build" (12s), with colored status icons and no Success/Failed/Pending text.',
			"Each row's icon color matches the same green/red/yellow scheme as the header pill -- the same visual language, not a separate one.",
		],
	});
}, 60000);

it("does not render a Checks section when the PR has no CI checks", async () => {
	api.ghViewPr.mockReset().mockResolvedValue(basePr());
	api.getPrChecksForPr.mockReset().mockResolvedValue(null);

	render(
		<PrDetailPanel repoFullName="acme/treq" prNumber={42} onClose={() => {}} />,
	);

	expect(await screen.findByText("Add CI status indicator to PR view")).toBeVisible();
	expect(screen.queryByText(/checks \(/i)).not.toBeInTheDocument();
	expect(screen.queryByRole("button", { name: /CI/ })).not.toBeInTheDocument();

	await captureDocument(document, {
		name: "pr-detail-checks-02-no-checks",
		expectations: [
			"No CI status pill, 'Checks' section heading, or check rows are visible anywhere in the PR detail panel.",
			"The PR title, branch refs, and description render normally, immediately followed by the description box with no gap for an empty checks section.",
		],
	});
}, 60000);
