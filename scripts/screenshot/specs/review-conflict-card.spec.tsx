import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  checkAndRebaseWorkspaces,
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaces,
} from "../../../src/lib/api";
import { loadPendingReview } from "../../../src/lib/api-extra";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

// The inline conflict card only appears once the backend's conflict regions
// survive the trip to the frontend. This drives the real flow -- treq's own
// auto-rebase onto a diverged default branch, open the Changes tab, pick the
// conflicted file -- so the card is rendered from real jj conflict markers.
it("captures the inline conflict card in the Changes tab", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);

  const user = userEvent.setup();

  // Incidental background state: the workspace itself isn't what's being
  // verified, the conflict card inside its Review tab is.
  const workspaceId = await createWorkspace(repoPath, "feat/conflict-card");
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error("Workspace not found");
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
  await createCommit(repoPath, workspaceId, "workspace conflicting change");

  writeWorkspaceFile(repoPath, "README.md", "main side\n");
  await createCommit(repoPath, null, "main conflicting change");

  await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);
  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

  const mount = render(<Dashboard />);
  await waitForGpuiConflict(workspaceId);

  await clickGpuiRow("feat/conflict-card");
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });

  const conflictsToggle = await screen.findByRole("button", {
    name: "Conflicts",
  });
  const conflictsSection = conflictsToggle.closest("div")?.parentElement;
  if (!conflictsSection) throw new Error("Conflicts section not found");
  await user.click(await within(conflictsSection).findByTitle("README.md"));

  const conflictCard = await screen.findByText(/^Conflict 1 of 1$/);
  await captureDocument(document, {
    name: "review-conflict-card-01-inline-card",
    expectations: [
      'The sidebar has a red "Conflicts" section listing README.md, and the diff area shows a conflict card headed "Conflict 1 of 1" with a line range next to it.',
      'Three coloured badges label the conflict sections: "Side #1" (red), "Base" (amber/yellow) and "Side #2" (green), each sitting on its conflict marker line.',
      'The conflicting content lines are visible and tinted by side: "main side" red, "# Test Repository" amber, "workspace side" green.',
    ],
  });

  // Regression coverage: conflict comments must survive a reload, not just
  // live in the in-memory hook state (see conflict-review-comments-reload).
  const cardRoot = conflictCard.closest("[data-conflict-card]");
  if (!(cardRoot instanceof HTMLElement)) {
    throw new Error("Conflict card root not found");
  }
  await user.click(
    within(cardRoot).getByRole("button", { name: "Add comment" }),
  );
  const commentText = "Please double check side #2 resolves the header";
  const commentInput = await screen.findByPlaceholderText(/add a comment/i);
  await user.type(commentInput, commentText);
  await user.click(screen.getByRole("button", { name: "Add Comment" }));
  await screen.findByText(commentText);

  // Wait for the debounced persistence effect in useReview.ts to write the
  // conflict comment to the backend before simulating a reload.
  await waitFor(async () => {
    const pending = await loadPendingReview(repoPath, workspaceId);
    expect(pending?.conflict_comments?.length).toBeGreaterThan(0);
  });

  await captureDocument(document, {
    name: "review-conflict-card-02-comment-added",
    expectations: [
      'The conflict card shows a saved resolution note reading "Please double check side #2 resolves the header" instead of the empty "Add comment" state.',
    ],
  });

  // Simulate a page reload: unmount the whole tree (discarding the SWR
  // cache and the hook's one-time hydration guard) and mount fresh, exactly
  // like a real reload would force a new loadPendingReview fetch.
  mount.unmount();
  render(<Dashboard />);
  await waitForGpuiConflict(workspaceId);

  await clickGpuiRow("feat/conflict-card");
  await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
  await screen.findByRole("tab", { name: /^Changes/, selected: true });

  const conflictsToggleAfterReload = await screen.findByRole("button", {
    name: "Conflicts",
  });
  const conflictsSectionAfterReload =
    conflictsToggleAfterReload.closest("div")?.parentElement;
  if (!conflictsSectionAfterReload) {
    throw new Error("Conflicts section not found after reload");
  }
  await user.click(
    await within(conflictsSectionAfterReload).findByTitle("README.md"),
  );
  await screen.findByText(/^Conflict 1 of 1$/);

  // The regression: this comment used to be discarded on reload because it
  // lived only in useComments' in-memory state, never in the saved review.
  await screen.findByText(commentText);
  await captureDocument(document, {
    name: "review-conflict-card-03-comment-after-reload",
    expectations: [
      'After the app was unmounted and remounted (simulating a reload), the same conflict card still shows the saved resolution note "Please double check side #2 resolves the header" rather than reverting to an empty "Add comment" state.',
    ],
  });
}, 60000);
