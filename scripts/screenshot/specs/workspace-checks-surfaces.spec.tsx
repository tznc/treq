/**
 * Verifies every place Workspace Checks show up: the header Checks popover
 * (Treq checks with trust and run actions, plus the PR's GitHub checks), the
 * Logs tab check filter, the command palette, and the GitHub PR detail panel.
 */

import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { GitHubPanel } from "../../../src/components/GitHubPanel";
import { createWorkspace, runWorkflow, trustRepo } from "../../../src/lib/api";
import type { GhPullRequest, PrCiStatus } from "../../../src/lib/api-types";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  writeRepoFile,
} from "../../../test/utils";
import { captureDocument } from "../capture";

const {
  mockGetGitRemoteUrl,
  mockGhListPrs,
  mockGhViewPr,
  mockGetPrChecksForPr,
  mockGetCachedPrCiStatus,
} = vi.hoisted(() => ({
  mockGetCachedPrCiStatus: vi.fn(),
  mockGetGitRemoteUrl: vi.fn(),
  mockGhListPrs: vi.fn(),
  mockGhViewPr: vi.fn(),
  mockGetPrChecksForPr: vi.fn(),
}));

// Only the GitHub network calls are stubbed; the repo, workflows, trust and
// check runs all go through the real Rust commands.
vi.mock("../../../src/lib/api", async () => {
  const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
    "../../../src/lib/api",
  );
  return {
    ...actual,
    getGitRemoteUrl: mockGetGitRemoteUrl,
    ghListIssues: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
    ghListPrs: mockGhListPrs,
    ghViewPr: mockGhViewPr,
    getPrChecksForPr: mockGetPrChecksForPr,
    getCachedPrCiStatus: mockGetCachedPrCiStatus,
  };
});

const WORKFLOW = `
name: Lint and Test
on:
  workflow_dispatch: {}
jobs:
  lint:
    name: Lint
    steps:
      - name: Run lint
        run: "echo lint-ok"
  test:
    name: Test
    steps:
      - name: Run tests
        run: "echo running 12 tests; echo all passed"
`;

const DOCS_WORKFLOW = `
name: Docs
on:
  workflow_dispatch: {}
jobs:
  build-docs:
    name: Build docs
    steps:
      - name: Build site
        run: "echo building docs; echo docs-built"
`;

const BRANCH = "feat/checks-demo";

async function openWorkspaceWithChecks() {
  const { repoPath } = createTestRepo(false);
  await writeRepoFile(repoPath, ".treq/workflows/ci.yaml", WORKFLOW);
  await writeRepoFile(repoPath, ".treq/workflows/ci2.yaml", DOCS_WORKFLOW);
  openRepo(repoPath);
  await createWorkspace(repoPath, BRANCH);

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(BRANCH));
  await screen.findByTestId("show-workspace-header");
  const trigger = await screen.findByTestId(
    "workspace-checks-button",
    {},
    { timeout: 20_000 },
  );
  return { user, trigger };
}

it("captures checks in the header popover, Logs tab and command palette", async () => {
  mockGetCachedPrCiStatus.mockResolvedValue(null);
  const { user, trigger } = await openWorkspaceWithChecks();
  expect(trigger.textContent).toContain("not run");

  await captureDocument(document, {
    name: "workspace-checks-01-header-not-run",
    expectations: [
      'The workspace header shows one outlined "Checks" button reading "Checks not run" with a neutral dot icon.',
      "There is no standalone Checks tab in the workspace tab row and no separate CI pill or play button.",
    ],
  });

  await user.click(trigger);
  const popover = await screen.findByTestId("checks-popover");
  await within(popover).findByRole("button", { name: "Trust Repository" });

  await captureDocument(document, {
    name: "workspace-checks-02-popover-untrusted",
    expectations: [
      'An open popover below the Checks button has a "Treq checks" section listing "Lint and Test" and "Docs", each reading "not run".',
      'An amber "Trust Repository" button sits next to a disabled "Run all" button.',
      'A "View logs" link sits at the right of the "Treq checks" title.',
    ],
  });

  await user.click(
    within(popover).getByRole("button", { name: "Trust Repository" }),
  );
  const runAll = within(popover).getByRole("button", {
    name: "Run all checks",
  });
  await waitFor(() =>
    expect((runAll as HTMLButtonElement).disabled).toBe(false),
  );
  await user.click(runAll);
  await waitFor(() => expect(trigger.dataset.checksState).toBe("passed"), {
    timeout: 30_000,
  });

  await captureDocument(document, {
    name: "workspace-checks-03-popover-passed",
    expectations: [
      'Both popover rows show a green check icon and read "run #N passed", each with a re-run icon button.',
      'The header button reads "Checks 2/2" with a green check icon.',
      'The "Trust Repository" button is gone and "Run all" is enabled.',
    ],
  });

  await user.click(within(popover).getByRole("button", { name: "View logs" }));
  await screen.findByText("all passed", {}, { timeout: 20_000 });
  await screen.findByText("docs-built", {}, { timeout: 20_000 });

  await captureDocument(document, {
    name: "workspace-checks-04-logs-all-checks",
    expectations: [
      'The Logs filter bar has the level filter, a check dropdown reading "All checks" and a search box, with no run, trust or setup buttons.',
      'The log feed shows lines from both workflows: "all passed" (jobs lint/test) and "docs-built" (job build-docs).',
      "The Checks popover is closed.",
    ],
  });

  await user.selectOptions(
    screen.getByRole("combobox", { name: "Filter by check" }),
    "ci2.yaml",
  );
  await waitFor(() => expect(screen.queryByText("all passed")).toBeNull());

  await captureDocument(document, {
    name: "workspace-checks-05-logs-one-check",
    expectations: [
      'The check dropdown reads "Docs" with no run button beside it.',
      'The log feed only shows lines from job build-docs, such as "docs-built"; no lint or test output.',
    ],
  });

  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByTestId("modal");
  await screen.findByText("Run checks…");
  await screen.findByText("Run check: Lint and Test");

  await captureDocument(document, {
    name: "workspace-checks-06-command-palette",
    // The checks commands sit below the fold of the palette list.
    scrollIntoView: "text=Run check: Lint and Test",
    expectations: [
      'The command palette lists "Run checks…" and "Run check: Lint and Test" for the open workspace.',
    ],
  });
}, 120000);

const CI_STATUS: PrCiStatus = {
  state: "failure",
  total: 3,
  passed: 2,
  failed: 1,
  pending: 0,
  checks: [
    { name: "build", bucket: "pass", link: "https://x/1", duration_secs: 72 },
    { name: "e2e", bucket: "fail", link: "https://x/2", duration_secs: 356 },
    { name: "lint", bucket: "pass", link: "https://x/3", duration_secs: 12 },
  ],
};

it("captures Treq and GitHub checks together in the header popover", async () => {
  mockGetCachedPrCiStatus.mockResolvedValue(CI_STATUS);
  const { user, trigger } = await openWorkspaceWithChecks();
  await within(trigger).findByText("GH 2/3");

  await user.click(trigger);
  const popover = await screen.findByTestId("checks-popover");
  await within(popover).findByRole("region", { name: "GitHub checks" });

  await captureDocument(document, {
    name: "workspace-checks-07-popover-with-github",
    expectations: [
      'The header button reads "Checks not run" followed by red "GH 2/3", with a red X-circle icon.',
      'The popover shows a "Treq checks" section (two workflows, Trust Repository, Run all) above a "GitHub checks · 2/3" section.',
      'The GitHub section lists "e2e" first with a red icon and 5m 56s, then "build" and "lint" with green icons.',
    ],
  });
}, 120000);

it("captures the Treq checks section in the GitHub PR detail panel", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  await writeRepoFile(repoPath, ".treq/workflows/ci.yaml", WORKFLOW);
  const workspaceId = await createWorkspace(repoPath, BRANCH);
  await trustRepo(repoPath);
  const workspace = (
    await import("../../../src/lib/api").then((m) => m.getWorkspaces(repoPath))
  ).find((w) => w.id === workspaceId);
  await runWorkflow(
    repoPath,
    "ci.yaml",
    workspaceId,
    workspace?.workspace_path ?? repoPath,
  );

  const pr: GhPullRequest = {
    number: 51,
    title: "Add checks demo",
    state: "OPEN",
    url: "https://github.com/treq-dev/treq/pull/51",
    body: "Runs lint and tests through treq checks.",
    author: { login: "octocat", avatar_url: null },
    labels: [],
    head_ref_name: BRANCH,
    base_ref_name: defaultBranch,
    merge_state_status: "CLEAN",
    created_at: "2026-10-12T10:00:00Z",
    updated_at: "2026-10-12T10:00:00Z",
    comments: [],
    is_draft: false,
  };
  mockGetGitRemoteUrl.mockResolvedValue({
    owner: "treq-dev",
    repo: "treq",
    full_name: "treq-dev/treq",
  });
  mockGhListPrs.mockResolvedValue({ items: [pr], hasMore: false });
  mockGhViewPr.mockResolvedValue(pr);
  mockGetPrChecksForPr.mockResolvedValue(null);

  const user = userEvent.setup();
  render(
    <GitHubPanel
      repoPath={repoPath}
      onOpenSettings={vi.fn()}
      onOpenWorkspace={vi.fn()}
    />,
  );
  await user.click(await screen.findByRole("tab", { name: /pull requests/i }));
  await user.click(await screen.findByText("Add checks demo"));
  const section = await screen.findByTestId(
    "pr-treq-checks",
    {},
    { timeout: 20_000 },
  );
  await within(section).findByRole(
    "button",
    { name: "Re-run Lint and Test" },
    { timeout: 20_000 },
  );

  await captureDocument(document, {
    name: "workspace-checks-08-pr-detail",
    scrollIntoView: '[data-testid="pr-treq-checks"]',
    expectations: [
      'The PR detail panel has a "Treq checks" section listing the "Lint and Test" workflow with a passed status and a Re-run button.',
      'The section header has an "Open in Logs" button.',
    ],
  });
}, 120000);
