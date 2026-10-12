/**
 * Verifies agent review of Linear content: review comments (with suggested
 * changes) on an issue description, on a Linear comment, on a project
 * description and on a document; applying a suggestion back to Linear; and
 * starting a review, which writes the snapshot and opens an agent session.
 * Only the Linear HTTP API is stubbed; review comments live in the real
 * local DB and the snapshot is written by the real backend.
 */

import fs from "node:fs";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import { expect, it, onTestFinished, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces, resolveAgentReviewComment } from "../../../src/lib/api";
import type {
	LinearComment,
	LinearDocument,
	LinearIssue,
	LinearProject,
} from "../../../src/lib/api-linear";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
	createTestRepo,
	openRepo,
	seedAgentReviewComment,
} from "../../../test/utils";
import { captureDocument } from "../capture";

const fixtures = vi.hoisted(() => {
	const issue: LinearIssue = {
		id: "issue-1",
		identifier: "ENG-42",
		title: "Cache search results per workspace",
		description:
			"Search re-indexes on every keystroke.\n\nCache results per workspace and invalidate on file save.\n\nAcceptance: search returns in under 50ms.",
		state: { name: "Todo", type: "unstarted" },
		labels: ["performance"],
		branch_name: "eng-42-cache-search",
		parent_id: null,
		sub_issue_ids: [],
		url: "https://linear.app/acme/issue/ENG-42",
	};
	const issueComments: LinearComment[] = [
		{
			id: "issue-comment-1",
			body: "We should just cache forever, invalidation is overkill.",
			user: { id: "u-alice", name: "Alice" },
			created_at: "2026-09-30T10:00:00Z",
		},
	];
	const project: LinearProject = {
		id: "project-1",
		name: "Search Revamp",
		description:
			"Rebuild search ranking.\n\nShip by end of Q4 with no regressions.",
		state: "started",
		target_date: "2026-12-15",
		progress: 0.4,
		url: "https://linear.app/acme/project/search-revamp",
		lead: { id: "u-alice", name: "Alice" },
	};
	const projectComments: LinearComment[] = [
		{
			id: "project-comment-1",
			body: "Do we have a baseline for regressions?",
			user: { id: "u-bob", name: "Bob" },
			created_at: "2026-09-29T09:00:00Z",
			quoted_text: "no regressions",
		},
	];
	const document: LinearDocument = {
		id: "doc-1",
		title: "Ranking RFC",
		content:
			"# Ranking RFC\n\nWe rank by recency only.\n\nOpen question: how do we weight exact matches?",
		url: "https://linear.app/acme/document/ranking-rfc",
		updated_at: "2026-09-28T12:00:00Z",
	};
	return { issue, issueComments, project, projectComments, document };
});

const linearMocks = vi.hoisted(() => ({
	linearApplyReviewSuggestion: vi.fn(),
}));

vi.mock("../../../src/lib/api-linear", async (importOriginal) => ({
	...(await importOriginal<typeof import("../../../src/lib/api-linear")>()),
	linearListTeams: vi.fn().mockResolvedValue([]),
	linearGetViewer: vi.fn().mockResolvedValue({ id: "u-me", name: "Ty" }),
	// Fresh copies, as Linear would return, so a refetch shows edited text.
	linearListIssues: vi.fn(async () => [{ ...fixtures.issue }]),
	linearListIssueComments: vi.fn(async () => fixtures.issueComments),
	linearListProjects: vi.fn(async () => [fixtures.project]),
	linearListProjectComments: vi.fn(async () => fixtures.projectComments),
	linearListProjectDocuments: vi.fn(async () => [fixtures.document]),
	linearListDocumentComments: vi.fn(async () => []),
	linearApplyReviewSuggestion: linearMocks.linearApplyReviewSuggestion,
}));

it("reviews Linear issues, comments, projects and documents with the agent", async () => {
	onTestFinished(installFakeAgents());
	const { repoPath } = createTestRepo(false);
	openRepo(repoPath);
	// Opens the local DB so the seeded rows below land in a migrated table.
	await getWorkspaces(repoPath);

	// What `treq agent-review add` stores for a Linear target: the snapshot
	// file, its line range, and the exact text of that range.
	seedAgentReviewComment(repoPath, {
		targetType: "linear_issue",
		targetId: "issue-1",
		filePath: "body.md",
		startLine: 5,
		commentText:
			"50ms is not measurable as written: say which percentile and which repo size.",
		quotedText: "Acceptance: search returns in under 50ms.",
		suggestedReplacement:
			"Acceptance: p95 search latency is under 50ms on a 10k-file repo.",
	});
	seedAgentReviewComment(repoPath, {
		targetType: "linear_issue",
		targetId: "issue-1",
		filePath: "comments/issue-comment-1.md",
		startLine: 1,
		commentText:
			"Caching forever serves stale results after a file save, which the description rules out. This disagreement is unresolved.",
		quotedText: "We should just cache forever, invalidation is overkill.",
	});
	seedAgentReviewComment(repoPath, {
		targetType: "linear_project",
		targetId: "project-1",
		filePath: "body.md",
		startLine: 3,
		commentText:
			"Bob asked for a regression baseline and nobody answered; the goal needs one to be checkable.",
		quotedText: "Ship by end of Q4 with no regressions.",
		suggestedReplacement:
			"Ship by end of Q4 with no regression against the September ranking benchmark.",
	});
	seedAgentReviewComment(repoPath, {
		targetType: "linear_document",
		targetId: "doc-1",
		filePath: "body.md",
		startLine: 3,
		commentText:
			"Recency-only ranking contradicts the open question below about exact matches.",
		quotedText: "We rank by recency only.",
		suggestedReplacement:
			"We rank by exact match first, then by recency.",
	});

	// Stands in for Linear: write the suggestion into the issue, then resolve
	// the review comment the way the real command does.
	linearMocks.linearApplyReviewSuggestion.mockImplementation(
		async (repo: string, commentId: string) => {
			fixtures.issue.description = fixtures.issue.description?.replace(
				"Acceptance: search returns in under 50ms.",
				"Acceptance: p95 search latency is under 50ms on a 10k-file repo.",
			);
			await resolveAgentReviewComment(repo, commentId);
		},
	);

	const user = userEvent.setup();
	render(<Dashboard />);

	await user.click(await screen.findByTestId("linear-sidebar-item"));
	await user.click(await screen.findByText(fixtures.issue.title));
	const expanded = await screen.findByTestId("linear-issue-expanded");
	await within(expanded).findByText(/50ms is not measurable/);
	await within(expanded).findByText(/Caching forever serves stale results/);
	expect(
		within(expanded).getByRole("button", { name: "Review with agent" }),
	).toBeInTheDocument();

	await captureDocument(document, {
		name: "linear-agent-review-01-issue",
		expectations: [
			"The expanded ENG-42 issue shows its description on the left with a 'Review with agent' button and a '2 agent review comments' count above it; the Acceptance line is highlighted violet.",
			"The right column has an 'Agent review' card on the Description whose suggested change shows the Acceptance line removed (red) and the p95 rewrite added (green), with an 'Apply to Linear' button.",
			"Under Alice's Linear comment sits a second violet card labelled 'Comment by Alice' analysing that comment, with Resolve and Delete but no Apply.",
		],
	});

	await user.click(
		within(
			within(expanded).getByTestId("linear-agent-review-body"),
		).getByRole("button", { name: "Apply to Linear" }),
	);
	await waitFor(() =>
		expect(
			within(expanded).queryByTestId("linear-agent-review-body"),
		).toBeNull(),
	);
	// With the card gone, this can only match the refreshed description.
	await within(expanded).findByText(
		"Acceptance: p95 search latency is under 50ms on a 10k-file repo.",
	);
	expect(linearMocks.linearApplyReviewSuggestion).toHaveBeenCalledTimes(1);

	await captureDocument(document, {
		name: "linear-agent-review-02-issue-applied",
		expectations: [
			"The issue description's Acceptance line now reads 'p95 search latency is under 50ms on a 10k-file repo.'",
			"The Description review card is gone; only the card on Alice's comment remains and the count reads '1 agent review comment'.",
		],
	});

	await user.click(await screen.findByRole("tab", { name: "Projects" }));
	const projectDetail = await screen.findByTestId("linear-project-detail");
	await within(projectDetail).findByText(/regression baseline/);

	await captureDocument(document, {
		name: "linear-agent-review-03-project",
		expectations: [
			"The Search Revamp project detail shows its description with 'no regressions' highlighted yellow (Bob's Linear comment) inside a sentence the agent card covers.",
			"An 'Agent review' card above Bob's comment explains the unanswered baseline question and suggests a rewritten goal line, with 'Apply to Linear'.",
			"A 'Review with agent' button sits above the project description.",
		],
	});

	await user.click(
		within(projectDetail).getByTestId("linear-document-item-doc-1"),
	);
	const dialog = await screen.findByRole("dialog");
	await within(dialog).findByText(/Recency-only ranking contradicts/);

	await captureDocument(document, {
		name: "linear-agent-review-04-document",
		expectations: [
			"The Ranking RFC document dialog shows 'We rank by recency only.' highlighted violet.",
			"An 'Agent review' card in the right column flags the contradiction and suggests 'We rank by exact match first, then by recency.' as a red/green diff.",
		],
	});

	await user.click(
		within(dialog).getByRole("button", { name: "Review with agent" }),
	);

	const snapshotDir = path.join(
		repoPath,
		".treq",
		"linear-review",
		"linear_document",
		"doc-1",
	);
	await waitFor(() =>
		expect(fs.existsSync(path.join(snapshotDir, "body.md"))).toBe(true),
	);
	expect(fs.readFileSync(path.join(snapshotDir, "body.md"), "utf8")).toBe(
		fixtures.document.content,
	);
	// The prompt is many lines long, so its first line scrolls out of view;
	// its closing rules and the fake agent's launch lines are what remain.
	const pane = await screen.findByTestId("workspace-terminal-pane");
	await within(pane).findByText(/fake-agent: claude/, {}, { timeout: 15000 });
	expect(pane.querySelector(".xterm-rows")?.textContent).toContain(
		"Do not edit files or change anything in Linear yourself.",
	);

	await captureDocument(document, {
		name: "linear-agent-review-05-review-started",
		expectations: [
			"An 'AI Review' agent session tab is open in the home repository's terminal pane.",
			"The terminal shows the tail of the Linear review prompt (the --suggestion example and the Rules list) followed by 'fake-agent: claude' and 'mode: acceptEdits'.",
		],
	});
}, 90000);
