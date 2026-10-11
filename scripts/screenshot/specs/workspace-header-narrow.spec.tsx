/**
 * Verifies that the workspace header stays legible when the main area is
 * narrow: the right-hand actions (Push, Merge, Details)
 * must not overlap the branch name or target branch selector on the left.
 */

import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { render, screen, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/narrow-header-demo";

// getCachedPrInfo reads the Rust PR-status cache, which the sandbox never
// fills; stub it so the header renders its View PR controls.
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

it("keeps the workspace header actions clear of the branch controls when narrow", async () => {
  mockGetCachedPrInfo.mockResolvedValue({
    number: 99,
    title: "Narrow header",
    state: "OPEN",
    url: "https://github.com/treq-dev/treq/pull/99",
    head_ref_name: BRANCH_NAME,
    base_ref_name: "main",
    merge_state_status: "CLEAN",
  });
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, BRANCH_NAME);

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await screen.findByText(BRANCH_NAME));
  const header = await screen.findByTestId("show-workspace-header");
  expect(
    await within(header).findByRole("button", { name: /^push$/i }),
  ).toBeVisible();

  await captureDocument(document, {
    name: "workspace-header-narrow-01-900px",
    viewport: { width: 900, height: 600 },
    expectations: [
      "In the workspace header, no button or label is drawn on top of another: the branch name and target branch selector are fully readable.",
      "The right-hand actions (Push, Merge, Details, more) are fully visible, moving to a second header row if the first row has no room.",
    ],
  });

  await captureDocument(document, {
    name: "workspace-header-narrow-02-1440px",
    expectations: [
      "At full width the workspace header is a single row: branch controls on the left, Push / Merge / Details on the right.",
    ],
  });
}, 60000);
