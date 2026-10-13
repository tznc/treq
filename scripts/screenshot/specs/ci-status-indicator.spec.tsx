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

const BRANCH_NAME = "feat/ci-status-demo";

// Merge queue is irrelevant to this flow and would otherwise need Supabase
// stubbed too -- turn it off so the header only shows the View PR / Create PR
// affordances this spec cares about.
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
      {
        name: "build",
        bucket: "pass",
        link: "https://github.com/x/1",
        duration_secs: 5 * 60 + 56,
      },
      {
        name: "lint",
        bucket: "pass",
        link: "https://github.com/x/2",
        duration_secs: 12,
      },
    ],
    ...overrides,
  };
}

function stubPr() {
  mockGetCachedPrInfo.mockResolvedValue({
    number: 42,
    title: "CI status demo",
    state: "OPEN",
    url: "https://github.com/treq-dev/treq/pull/42",
    head_ref_name: BRANCH_NAME,
    base_ref_name: "main",
    merge_state_status: "CLEAN",
  });
  mockGetGitRemoteUrl.mockResolvedValue(REMOTE_INFO);
}

// Real repo + workspace + push, driven through the app's own affordances --
// the View PR / CI status buttons are gated on the branch existing on remote.
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
    "Workspace commit for CI status demo",
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

it("captures CI passing in the header Checks button", async () => {
  const user = userEvent.setup();
  stubPr();
  mockGetCachedPrCiStatus.mockResolvedValue(ciStatus());

  await setupPushedWorkspace(user);

  await screen.findByRole("button", { name: /view pr/i });
  await screen.findByRole("button", { name: "Checks: GitHub 2/2" });
  await captureDocument(document, {
    name: "ci-status-indicator-01-success",
    expectations: [
      'The workspace header has an outlined "Checks" button with a green check-circle icon and green "GH 2/2" text.',
      'The Checks button and the green "View PR" button sit on the same header row, not overlapping or wrapped.',
    ],
  });
}, 120000);

it("captures CI failing in the header Checks button", async () => {
  const user = userEvent.setup();
  stubPr();
  mockGetCachedPrCiStatus.mockResolvedValue(
    ciStatus({
      state: "failure",
      passed: 1,
      failed: 1,
      checks: [
        {
          name: "build",
          bucket: "pass",
          link: "https://github.com/x/1",
          duration_secs: 12,
        },
        {
          name: "test",
          bucket: "fail",
          link: "https://github.com/x/2",
          duration_secs: 5 * 60 + 56,
        },
      ],
    }),
  );

  await setupPushedWorkspace(user);

  await screen.findByRole("button", { name: /view pr/i });
  await screen.findByRole("button", { name: "Checks: GitHub 1/2" });
  await captureDocument(document, {
    name: "ci-status-indicator-02-failure",
    expectations: [
      'The header "Checks" button shows a red X-circle icon and red "GH 1/2" text.',
      "The failure state is clearly a different color (red) than a passing state would be (green).",
    ],
  });
}, 120000);

it("captures CI still running (pending) in the header Checks button", async () => {
  const user = userEvent.setup();
  stubPr();
  mockGetCachedPrCiStatus.mockResolvedValue(
    ciStatus({
      state: "pending",
      passed: 1,
      pending: 1,
      checks: [
        {
          name: "build",
          bucket: "pass",
          link: "https://github.com/x/1",
          duration_secs: 12,
        },
        {
          name: "test",
          bucket: "pending",
          link: "https://github.com/x/2",
          duration_secs: 45,
        },
      ],
    }),
  );

  await setupPushedWorkspace(user);

  await screen.findByRole("button", { name: /view pr/i });
  await screen.findByRole("button", { name: "Checks: GitHub 1/2" });
  await captureDocument(document, {
    name: "ci-status-indicator-03-pending",
    expectations: [
      'The header "Checks" button shows a spinning loader icon and yellow "GH 1/2" text.',
      "The pending state is visually distinct in color (yellow) from both the green success and red failure states.",
    ],
  });
}, 120000);

it("captures the CI checks dropdown with time-taken instead of job status text", async () => {
  const user = userEvent.setup();
  stubPr();
  mockGetCachedPrCiStatus.mockResolvedValue(
    ciStatus({
      state: "failure",
      passed: 1,
      failed: 1,
      pending: 1,
      total: 3,
      checks: [
        {
          name: "build",
          bucket: "pass",
          link: "https://github.com/x/1",
          duration_secs: 12,
        },
        {
          name: "lint",
          bucket: "pending",
          link: "https://github.com/x/3",
          duration_secs: 45,
        },
        {
          name: "test",
          bucket: "fail",
          link: "https://github.com/x/2",
          duration_secs: 5 * 60 + 56,
        },
      ],
    }),
  );

  await setupPushedWorkspace(user);

  await user.click(
    await screen.findByRole("button", { name: "Checks: GitHub 1/3" }),
  );
  const section = within(
    await screen.findByRole("region", { name: "GitHub checks" }),
  );
  const checkButtons = section.getAllByRole("button", {
    name: /^(test|lint|build)/i,
  });
  expect(checkButtons.map((button) => button.textContent)).toEqual([
    "test5m 56s",
    "lint45s",
    "build12s",
  ]);
  expect(screen.queryByText("Failed")).not.toBeInTheDocument();
  expect(screen.queryByText("Success")).not.toBeInTheDocument();

  await captureDocument(document, {
    name: "ci-status-indicator-04-dropdown-duration",
    expectations: [
      'An open Checks popover lists rows as failed → pending → success: "test" (5m 56s), then "lint" (45s), then "build" (12s).',
      "Each row still shows a colored status icon (red, yellow, green) so status is conveyed by icon, not label text.",
      'The popover section title reads "GitHub checks · 1/3" and the popover opens below the header Checks button.',
    ],
  });
}, 120000);
