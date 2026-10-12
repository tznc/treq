/**
 * Verifies every place Workspace Checks can be run from now that there is no
 * standalone Checks tab: the workspace header, the Logs tab, the command
 * palette, and the GitHub PR detail panel.
 */

import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { GitHubPanel } from "../../../src/components/GitHubPanel";
import { createWorkspace, runWorkflow, trustRepo } from "../../../src/lib/api";
import type { GhPullRequest } from "../../../src/lib/api-types";
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
} = vi.hoisted(() => ({
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

const BRANCH = "feat/checks-demo";

it("captures checks in the workspace header, Logs tab and command palette", async () => {
  const { repoPath } = createTestRepo(false);
  await writeRepoFile(repoPath, ".treq/workflows/ci.yaml", WORKFLOW);
  openRepo(repoPath);
  await createWorkspace(repoPath, BRANCH);

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(BRANCH));
  await screen.findByTestId("show-workspace-header");

  const indicator = await screen.findByTestId(
    "workspace-checks-indicator",
    {},
    { timeout: 20_000 },
  );
  expect(indicator.textContent).toContain("Checks not run");

  await captureDocument(document, {
    name: "workspace-checks-01-header-not-run",
    expectations: [
      'The workspace header shows a compact "Checks not run" summary with a play (run all) button next to it.',
      "There is no standalone Checks tab in the workspace tab row.",
    ],
  });

  await user.click(within(indicator).getByRole("button", { name: /Checks/ }));
  const checks = await screen.findByTestId("workspace-checks");
  await within(checks).findByRole("button", { name: "Trust Repository" });

  await captureDocument(document, {
    name: "workspace-checks-02-logs-untrusted",
    expectations: [
      "The Logs tab is selected and a checks panel sits above the log browser.",
      'The panel asks to trust the repository with a "Trust Repository" button before checks can run.',
    ],
  });

  await user.click(
    within(checks).getByRole("button", { name: "Trust Repository" }),
  );
  const run = await within(checks).findByRole("button", {
    name: "Run Lint and Test",
  });
  await waitFor(() => expect((run as HTMLButtonElement).disabled).toBe(false));
  await user.click(run);
  await within(checks).findByRole(
    "button",
    { name: "Re-run Lint and Test" },
    { timeout: 30_000 },
  );
  await waitFor(() => expect(indicator.dataset.state).toBe("passed"), {
    timeout: 20_000,
  });
  await screen.findByText("all passed", {}, { timeout: 20_000 });

  await captureDocument(document, {
    name: "workspace-checks-03-logs-passed",
    expectations: [
      'The checks panel lists the "Lint and Test" workflow with a passed status and a Re-run button, plus its Lint and Test jobs.',
      'The log browser below shows output from the run, including "all passed".',
      'The header summary now reads "Checks 1/1" with a passed icon.',
    ],
  });

  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByTestId("modal");
  await screen.findByText("Run checks…");
  await screen.findByText("Run check: Lint and Test");

  await captureDocument(document, {
    name: "workspace-checks-04-command-palette",
    // The checks commands sit below the fold of the palette list.
    scrollIntoView: "text=Run check: Lint and Test",
    expectations: [
      'The command palette lists "Run checks…" and "Run check: Lint and Test" for the open workspace.',
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
    name: "workspace-checks-05-pr-detail",
    scrollIntoView: '[data-testid="pr-treq-checks"]',
    expectations: [
      'The PR detail panel has a "Treq checks" section listing the "Lint and Test" workflow with a passed status and a Re-run button.',
      'The section header has an "Open in Logs" button.',
    ],
  });
}, 120000);
