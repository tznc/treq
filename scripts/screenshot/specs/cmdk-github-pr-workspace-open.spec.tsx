import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { render, screen } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/pr-cmdk-demo";

// getCachedPrInfo reads from the Rust PR-status cache, which isn't populated
// in the sandbox. Stub it at the src/lib/api boundary (the same seam
// merge-queue.spec.tsx uses) so usePrInfoViaGh can resolve a real PrInfo without
// touching the `gh` subprocess. Everything else -- repo, workspace, Dashboard,
// Rust dispatch -- stays real.
const { mockGetCachedPrInfo } = vi.hoisted(() => ({
  mockGetCachedPrInfo: vi.fn(),
}));

vi.mock("../../../src/lib/api", async () => {
  const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
    "../../../src/lib/api",
  );
  return {
    ...actual,
    getCachedPrInfo: mockGetCachedPrInfo,
    startPrStatusPolling: vi.fn(async () => undefined),
    refreshPrBranchStatus: vi.fn(async () => undefined),
    stopPrStatusPolling: vi.fn(async () => undefined),
    refreshPrStatuses: vi.fn(async () => undefined),
  };
});

async function setupSelectedWorkspace() {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, BRANCH_NAME);

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow(BRANCH_NAME);
  await screen.findByTestId("show-workspace-header");
  return user;
}

it("captures the workspace PR commands in the command palette when a PR exists", async () => {
  mockGetCachedPrInfo.mockResolvedValue({
    number: 99,
    title: "Add cmdk PR shortcuts",
    state: "OPEN",
    url: "https://github.com/treq-dev/treq/pull/99",
    head_ref_name: BRANCH_NAME,
    base_ref_name: "main",
    merge_state_status: "CLEAN",
  });

  const user = await setupSelectedWorkspace();

  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByTestId("modal");

  const openInBrowser = await screen.findByText("Open Workspace PR in Browser");
  // Distinct from "Open Workspace PR in Browser" -- exact match so it doesn't
  // also match as a substring of the browser-labeled item above.
  const openInApp = await screen.findByText("Open Workspace PR", {
    exact: true,
  });
  expect(openInBrowser).toBeInTheDocument();
  expect(openInApp).toBeInTheDocument();
  await screen.findByText("Open PR #99 on GitHub");
  await screen.findByText("Navigate to PR #99");

  await captureDocument(document, {
    name: "cmdk-github-pr-workspace-open-01-with-pr",
    // The two new items are last in the list, below the fold of the cmdk
    // list's own overflow-y-auto scroll region -- scroll them into view so
    // the screenshot actually shows them instead of clipping them out.
    scrollIntoView: "text=Navigate to PR #99",
    expectations: [
      'The open command palette lists an "Open Workspace PR in Browser" item describing "Open PR #99 on GitHub".',
      'The palette also lists a separate "Open Workspace PR" item describing "Navigate to PR #99".',
    ],
  });
}, 60000);

it("hides the workspace PR commands when the workspace has no PR", async () => {
  // gh positively reports there's no PR for this branch.
  mockGetCachedPrInfo.mockResolvedValue(null);

  const user = await setupSelectedWorkspace();

  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByTestId("modal");
  // A command that's always present once the palette is open, to prove the
  // palette actually rendered rather than the queries below vacuously passing.
  await screen.findByText("Go to Home");

  expect(
    screen.queryByText("Open Workspace PR in Browser"),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByText("Open Workspace PR", { exact: true }),
  ).not.toBeInTheDocument();

  await captureDocument(document, {
    name: "cmdk-github-pr-workspace-open-02-without-pr",
    // Scroll to the last real command so the screenshot proves nothing
    // (i.e. no PR item) follows it, not just that the visible top of the
    // list lacks one.
    scrollIntoView: "text=New Shell Terminal",
    expectations: [
      'The open command palette does NOT list any "Open Workspace PR" items, since this workspace has no associated GitHub PR.',
      'The last visible command is "New Shell Terminal" ("Create a new shell session"), with nothing about a PR after it.',
    ],
  });
}, 60000);
