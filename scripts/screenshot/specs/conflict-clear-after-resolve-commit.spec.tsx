import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  newCommitWithParents,
  openRepo,
  resolveChangeId,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  checkAndRebaseWorkspaces,
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaceStatus,
  getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

type ConflictFixture = {
  slug: string;
  repoPath: string;
  branchName: string;
  workspaceId: number;
  workspacePath: string;
  conflictFiles: string[];
  resolvedContent: Record<string, string>;
};

async function createWorkspaceOnBranch(repoPath: string, branchName: string) {
  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace ${branchName} not found`);
  return {
    workspaceId,
    workspacePath: resolveWorkspacePath(repoPath, workspace.workspace_path),
  };
}

async function assertConflicted(
  repoPath: string,
  workspaceId: number,
  files: string[],
) {
  const status = await getWorkspaceStatus(repoPath, workspaceId);
  expect(status.has_conflicts).toBe(true);
  for (const file of files) {
    expect(status.conflicted_files).toContain(file);
  }
}

async function navigateToReview(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  await clickGpuiRow(branchName);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
}

async function resolveAndCommitViaUi(
  user: ReturnType<typeof userEvent.setup>,
  fixture: ConflictFixture,
  message: string,
) {
  for (const [relativePath, content] of Object.entries(
    fixture.resolvedContent,
  )) {
    writeWorkspaceFile(fixture.workspacePath, relativePath, content);
  }

  // Re-enter Review so the file list picks up the resolved WC write.
  const overviewTab = await screen.findByRole("tab", {
    name: /^Code|^Overview/,
  });
  await user.click(overviewTab);
  await navigateToReview(user, fixture.branchName);

  await user.type(await screen.findByPlaceholderText("Message"), message);
  await user.click(screen.getByRole("button", { name: "Commit" }));
  await screen.findByText("Commit created");
}

async function assertConflictsCleared(fixture: ConflictFixture) {
  await waitFor(async () => {
    const status = await getWorkspaceStatus(
      fixture.repoPath,
      fixture.workspaceId,
    );
    expect(status.has_conflicts).toBe(false);
    expect(status.conflicted_files).toEqual([]);
  });

  await waitForGpuiConflict(fixture.workspaceId, false);

  await waitFor(() => {
    expect(
      screen.queryByRole("button", { name: "Conflicts" }),
    ).not.toBeInTheDocument();
  });
}

async function captureBeforeAfter(
  user: ReturnType<typeof userEvent.setup>,
  fixture: ConflictFixture,
  message: string,
) {
  render(<Dashboard />);
  await waitForGpuiConflict(fixture.workspaceId);
  await navigateToReview(user, fixture.branchName);
  await screen.findByRole("button", { name: "Conflicts" });

  await captureDocument(document, {
    name: `${fixture.slug}-01-before-resolve-commit`,
    expectations: [
      `The workspace sidebar shows a red conflict indicator for ${fixture.branchName}.`,
      'The Changes tab shows a red "Conflicts" section listing the conflicted file(s).',
      "A Commit button is visible so the conflict can be resolved by committing a fix.",
    ],
  });

  await resolveAndCommitViaUi(user, fixture, message);
  await assertConflictsCleared(fixture);

  await captureDocument(document, {
    name: `${fixture.slug}-02-after-resolve-commit`,
    expectations: [
      `The workspace sidebar no longer shows a conflict indicator for ${fixture.branchName}.`,
      'The Changes tab no longer shows a "Conflicts" section.',
      'A "Commit created" toast confirms the resolve commit succeeded.',
    ],
  });
}

// These two were the only permutations that failed on the first app-qa run.
// (delete/modify setup could not materialize a conflict; sequential failed to
// show the second conflict indicator without a remount. Kept as the narrowed
// regression set after fixing both.)

// ── Line-delete vs modify (replacement for the failed delete/modify setup) ───
it("clears line-delete vs modify conflict after resolve commit", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  writeWorkspaceFile(repoPath, "victim.txt", "line1\nline2\n");
  await createCommit(repoPath, null, "base victim");

  const branchName = "feat/clear-line-delete";
  const { workspaceId, workspacePath } = await createWorkspaceOnBranch(
    repoPath,
    branchName,
  );

  writeWorkspaceFile(workspacePath, "victim.txt", "line1\n");
  await createCommit(repoPath, workspaceId, "workspace deletes line2");
  const workspaceChangeId = resolveChangeId(workspacePath, "@-");

  writeWorkspaceFile(repoPath, "victim.txt", "line1\nline2 modified on main\n");
  await createCommit(repoPath, null, "main modifies line2");
  const mainChangeId = resolveChangeId(repoPath, "@-");

  newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);
  await assertConflicted(repoPath, workspaceId, ["victim.txt"]);

  const user = userEvent.setup();
  await captureBeforeAfter(
    user,
    {
      slug: "conflict-clear-line-delete-modify",
      repoPath,
      branchName,
      workspaceId,
      workspacePath,
      conflictFiles: ["victim.txt"],
      resolvedContent: { "victim.txt": "line1\nresolved line2\n" },
    },
    "resolve line-delete vs modify conflict",
  );
}, 90000);

// ── Two sequential conflicts resolved by successive commits ──────────────────
it("clears a second conflict that appears after the first was resolved", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);
  const branchName = "feat/clear-sequential";
  const { workspaceId, workspacePath } = await createWorkspaceOnBranch(
    repoPath,
    branchName,
  );

  writeWorkspaceFile(workspacePath, "first.txt", "ws first\n");
  await createCommit(repoPath, workspaceId, "workspace first");
  writeWorkspaceFile(repoPath, "first.txt", "main first\n");
  await createCommit(repoPath, null, "main first");
  await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);
  await assertConflicted(repoPath, workspaceId, ["first.txt"]);

  const user = userEvent.setup();
  const firstMount = render(<Dashboard />);
  await waitForGpuiConflict(workspaceId);
  await navigateToReview(user, branchName);
  await screen.findByRole("button", { name: "Conflicts" });
  await captureDocument(document, {
    name: "conflict-clear-sequential-01-first-conflict",
    expectations: [
      'The Changes tab shows a "Conflicts" section for first.txt.',
      "The workspace sidebar shows a conflict indicator before the first resolve.",
    ],
  });

  await resolveAndCommitViaUi(
    user,
    {
      slug: "conflict-clear-sequential",
      repoPath,
      branchName,
      workspaceId,
      workspacePath,
      conflictFiles: ["first.txt"],
      resolvedContent: { "first.txt": "resolved first\n" },
    },
    "resolve first conflict",
  );
  await assertConflictsCleared({
    slug: "conflict-clear-sequential",
    repoPath,
    branchName,
    workspaceId,
    workspacePath,
    conflictFiles: ["first.txt"],
    resolvedContent: { "first.txt": "resolved first\n" },
  });

  // Introduce a second conflict on a different file, then remount so the
  // sidebar/status queries pick up the new conflict (API writes alone do not
  // invalidate React Query while Dashboard stays mounted).
  writeWorkspaceFile(workspacePath, "second.txt", "ws second\n");
  await createCommit(repoPath, workspaceId, "workspace second");
  writeWorkspaceFile(repoPath, "second.txt", "main second\n");
  await createCommit(repoPath, null, "main second");
  await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);
  await assertConflicted(repoPath, workspaceId, ["second.txt"]);

  firstMount.unmount();
  render(<Dashboard />);

  await waitForGpuiConflict(workspaceId);
  await navigateToReview(user, branchName);
  await screen.findByRole("button", { name: "Conflicts" });
  await captureDocument(document, {
    name: "conflict-clear-sequential-02-second-conflict",
    expectations: [
      'After a new divergence, the Review "Conflicts" section is back for second.txt.',
      "The sidebar conflict indicator has reappeared for the second conflict.",
    ],
  });

  await resolveAndCommitViaUi(
    user,
    {
      slug: "conflict-clear-sequential",
      repoPath,
      branchName,
      workspaceId,
      workspacePath,
      conflictFiles: ["second.txt"],
      resolvedContent: { "second.txt": "resolved second\n" },
    },
    "resolve second conflict",
  );
  await assertConflictsCleared({
    slug: "conflict-clear-sequential",
    repoPath,
    branchName,
    workspaceId,
    workspacePath,
    conflictFiles: ["second.txt"],
    resolvedContent: { "second.txt": "resolved second\n" },
  });
  await captureDocument(document, {
    name: "conflict-clear-sequential-03-after-second-resolve",
    expectations: [
      "After resolving the second conflict, the sidebar conflict indicator is gone again.",
      'The Changes tab no longer shows a "Conflicts" section.',
      'A "Commit created" toast is visible for the second resolve commit.',
    ],
  });
}, 120000);
