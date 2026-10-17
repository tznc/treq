import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  commitWorkspaceFile,
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import * as api from "../../../src/lib/api";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";

// gh is an external service: the PR, remote and repo merge settings are
// stubbed; the landing plan and merges run against the real backend.
vi.mock("../../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../src/lib/api")>();
  return {
    ...actual,
    getCachedPrInfo: vi.fn(),
    getGitRemoteUrl: vi.fn(),
    ghRepoMergeSettings: vi.fn(),
  };
});

it("captures the merge landing plan locally and on GitHub", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);
  const parentId = await createWorkspace(repoPath, "feat/stack-a");
  await createWorkspace(repoPath, "feat/stack-b", "feat/stack-a");
  await createWorkspace(repoPath, "feat/stack-c", "feat/stack-b");
  const workspaces = await getWorkspaces(repoPath);
  const b = workspaces.find((w) => w.branch_name === "feat/stack-b")!;
  const a = workspaces.find((w) => w.id === parentId)!;
  await commitWorkspaceFile(
    repoPath,
    { id: a.id, path: a.workspace_path },
    "a.txt",
    "a",
    "Add a",
  );
  await commitWorkspaceFile(
    repoPath,
    { id: b.id, path: b.workspace_path },
    "b.txt",
    "b",
    "Add b",
  );

  vi.mocked(api.getGitRemoteUrl).mockResolvedValue({
    owner: "o",
    repo: "r",
    full_name: "o/r",
  });
  vi.mocked(api.getCachedPrInfo).mockResolvedValue({
    number: 42,
    title: "Stack B",
    state: "OPEN",
    url: "https://github.com/o/r/pull/42",
    head_ref_name: "feat/stack-b",
    base_ref_name: "feat/stack-a",
    merge_state_status: "DIRTY",
    is_draft: false,
  });
  vi.mocked(api.ghRepoMergeSettings).mockResolvedValue({
    default_method: "squash",
    allowed_methods: ["merge", "squash"],
  });

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement("feat/stack-b"));
  const header = await screen.findByTestId("show-workspace-header");
  const merge = await within(header).findByRole("button", {
    name: /^merge\.\.\.$/i,
  });
  await waitFor(() => expect(merge).not.toBeDisabled());
  await user.click(merge);

  await screen.findByText(
    "feat/stack-c is retargeted onto feat/stack-a (was feat/stack-b)",
  );
  await captureDocument(document, {
    name: "merge-preview-landing-01-local",
    expectations: [
      "A 'Locally' / 'On GitHub (#42)' toggle sits at the top of the card with 'Locally' selected, and a checked 'Push feat/stack-a to origin after merging' box sits above the merge button.",
      "The 'What lands where' panel shows feat/stack-b flowing down into feat/stack-a and notes feat/stack-c is retargeted.",
      `A 'Merge the whole stack into ${defaultBranch}' checkbox is shown unchecked.`,
    ],
  });

  await user.click(
    screen.getByRole("checkbox", {
      name: `Merge the whole stack into ${defaultBranch}`,
    }),
  );
  await screen.findByText(
    `feat/stack-a and feat/stack-b land in ${defaultBranch}.`,
  );
  await captureDocument(document, {
    name: "merge-preview-landing-02-rollup",
    expectations: [
      `The panel shows chips for feat/stack-a and feat/stack-b with a stack icon, flowing into ${defaultBranch}.`,
      `feat/stack-c is retargeted onto ${defaultBranch}.`,
    ],
  });

  await user.click(screen.getByRole("radio", { name: "On GitHub (#42)" }));
  await screen.findByRole("button", { name: "Squash and merge on GitHub" });
  await screen.findByText(/merge conflicts on GitHub/);
  await captureDocument(document, {
    name: "merge-preview-landing-03-github",
    expectations: [
      "'On GitHub (#42)' is selected and an amber warning says the PR has merge conflicts on GitHub.",
      "The primary button reads 'Squash and merge on GitHub' (the repo default).",
      "The landing panel shows 'PR #42 on GitHub' between feat/stack-b and feat/stack-a, and no rollup checkbox.",
    ],
  });
}, 90000);

it("captures syncing a PR that was already merged on GitHub", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const id = await createWorkspace(repoPath, "feat/landed");
  const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id)!;
  await commitWorkspaceFile(
    repoPath,
    { id: ws.id, path: ws.workspace_path },
    "landed.txt",
    "x",
    "Add landed",
  );
  vi.mocked(api.getGitRemoteUrl).mockResolvedValue({
    owner: "o",
    repo: "r",
    full_name: "o/r",
  });
  vi.mocked(api.getCachedPrInfo).mockResolvedValue({
    number: 51,
    title: "Landed",
    state: "MERGED",
    url: "https://github.com/o/r/pull/51",
    head_ref_name: "feat/landed",
    base_ref_name: ws.target_branch ?? "main",
    merge_state_status: null,
    is_draft: false,
  });

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement("feat/landed"));
  const header = await screen.findByTestId("show-workspace-header");
  const merge = await within(header).findByRole("button", {
    name: /^merge\.\.\.$/i,
  });
  await waitFor(() => expect(merge).not.toBeDisabled());
  await user.click(merge);

  await screen.findByRole("button", { name: "Sync merged PR" });
  await captureDocument(document, {
    name: "merge-preview-landing-04-merged-pr",
    expectations: [
      "A banner says PR #51 was already merged on GitHub, with a 'Sync merged PR' button.",
      "No 'On GitHub' toggle is shown, since the PR is no longer open.",
    ],
  });
}, 90000);
