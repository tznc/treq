// The Commits tab: the stacked workspace's conflicted commit, the Resolve
// conflicts dialog, an unfolded commit's diff, a commit's context menu and
// a commit large enough to defer its diff behind "Load diff".
//
// Commit rows are not observable elements: steps click their place in the
// 1400x900 window (the list starts under the conflict card).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("commits: conflicted stack, resolve dialog, unfolded diff", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Commits");
  await settle(page, 1500);
  await capture(page, {
    name: "04-commits-conflicted",
    title: "Commits tab: conflicted commit in a stack",
    expectations: [
      "Red '1 conflicted commit' card with Resolve conflicts…",
      "Top commit carries a red Conflict badge; base commits carry no per-row Immutable pill",
      "'On feat/event-ingest (read-only)' divider groups the base's commits",
    ],
  });

  await page.getByRole("button", { name: "Resolve conflicts…" }).click();
  const instructions = page.getByRole("textbox", {
    name: /^e\.g\. Prefer the workspace-side changes/,
  });
  await expect(instructions).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "04-commits-resolve-dialog",
    title: "Resolve conflicts dialog (in place)",
    expectations: [
      "Dialog lists the conflicted files and an instructions field",
      "Cancel and Resolve buttons aligned at the bottom right",
      "Backdrop dims the window evenly",
    ],
    issue: "The placeholder runs into the field's right edge",
  });
  await instructions.fill("Keep the event feed and the ingest owners' copy");
  await capture(page, {
    name: "04-commits-resolve-dialog-filled",
    title: "Resolve conflicts dialog with instructions",
    expectations: ["Typed instructions wrap inside the field"],
  });
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(instructions).toHaveCount(0);

  // Unfold the conflicted commit: its chevron, left of the first title. A
  // commit's fold state outlives the tab; "Edit description" shows unfolded.
  const editDescription = page.getByRole("button", { name: "Edit description" });
  // The first row sits a fixed distance under the banner's Resolve button.
  const resolve = await page.getByRole("button", { name: "Resolve conflicts…" }).boundingBox();
  if (!resolve) throw new Error("no Resolve conflicts… button");
  const firstRow = resolve.y + resolve.height + 73;
  if ((await editDescription.count()) === 0) await page.mouse.click(329, firstRow);
  await expect(editDescription).toBeVisible();
  await settle(page, 2000);
  await capture(page, {
    name: "04-commits-unfolded-diff",
    title: "Commits tab: conflicted commit unfolded",
    expectations: [
      "The commit's files show under it with their diffs or conflict state",
      "Action row fits on one line with one-word buttons: Undo, Revert, Move, Stash, Delete",
      "A pencil sits beside the description and another beside the timestamp; no per-commit Resolve",
    ],
  });
  await editDescription.hover();
  await settle(page, 1500);
  await capture(page, {
    name: "04-commits-edit-description-tooltip",
    title: "Edit description pencil tooltip",
    screen: true,
    rest: false,
    expectations: ["Hovering the pencil beside the description shows an 'Edit description' tooltip"],
  });
  await page.getByRole("button", { name: "Edit timestamp" }).hover();
  await settle(page, 1500);
  await capture(page, {
    name: "04-commits-edit-timestamp-tooltip",
    title: "Edit timestamp pencil tooltip",
    screen: true,
    rest: false,
    expectations: ["Hovering the pencil beside the timestamp shows an 'Edit timestamp' tooltip"],
  });
  await page.mouse.move(700, 700);
  await page.mouse.click(329, firstRow);
  await expect(editDescription).toHaveCount(0);
  await settle(page, 800);

  await page.mouse.click(600, firstRow, { button: "right" });
  await settle(page, 1000);
  await capture(page, {
    name: "04-commits-context-menu",
    title: "Commit row context menu",
    screen: true,
    rest: false,
    expectations: [
      "Right-click opens a menu at the pointer: Copy commit ID, Copy change ID, Copy message",
      "Below a separator, the row's actions (Edit description, Revert commit, …)",
      "Delete commit sits last, in red",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);

  await page.getByRole("button", { name: "Shift to now" }).hover();
  await settle(page, 1500);
  await capture(page, {
    name: "04-commits-shift-to-now-tooltip",
    title: "Shift to now tooltip",
    screen: true,
    rest: false,
    expectations: ["Tooltip explains the action and stays on screen at the right edge"],
    issue: "No tooltip appears on Shift to now",
  });
});

test("commits: large commit defers its diff", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Commits");
  await settle(page, 1500);
  // #388 is merged: its button reads Merged, Create PR doesn't show.
  await expect(page.getByRole("button", { name: "View PR" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Create PR" })).toHaveCount(0);
  // The work landed: the header offers Archive, not Merge….
  await expect(page.getByRole("button", { name: "Archive", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Merge…" })).toHaveCount(0);
  await capture(page, {
    name: "04-commits-plain-workspace",
    title: "Commits tab of a workspace without conflicts",
    expectations: [
      "Working copy row (uncommitted, amber) above two commits; newest +642",
      "No conflict card",
      "Header's PR button reads 'Merged' in purple with the merge icon; no 'Create PR'",
      "An outline Archive button stands where Merge… was",
    ],
  });
  // The working copy row, at the top of the list. Its fold state outlives
  // the tab, so it is read from its "View changes" action first.
  const viewChanges = page.getByRole("button", { name: "View changes" });
  if ((await viewChanges.count()) === 0) await page.mouse.click(329, 181);
  await expect(viewChanges).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "04-commits-working-copy",
    title: "Commits tab: working copy unfolded",
    expectations: [
      "View changes / Move changes / Delete changes actions in one row",
      "Uncommitted diffs of alerting.ts (M) and levels.ts (A)",
    ],
  });
  await page.mouse.click(329, 181);
  await expect(viewChanges).toHaveCount(0);
  await settle(page, 1000);
  // The newest commit, under the working copy (its fold state also
  // outlives the tab: its "Edit description" action shows when unfolded).
  const editDescription = page.getByRole("button", { name: "Edit description" });
  if ((await editDescription.count()) === 0) await page.mouse.click(329, 285);
  await expect(editDescription).toBeVisible();
  await settle(page, 2500);
  await capture(page, {
    name: "04-commits-load-diff",
    title: "Large commit: diff deferred behind Load diff",
    expectations: [
      "'This diff has 642 changed lines.' with a Load diff button",
      "Button aligned right in the file's footer row",
    ],
  });
  // "Load diff" is not observable: the right end of the file's footer row.
  await page.mouse.click(1333, 438);
  await settle(page, 2500);
  await capture(page, {
    name: "04-commits-load-diff-loaded",
    title: "Large commit: diff loaded",
    expectations: ["The JSON fixture's added lines render with line numbers and green background"],
  });
});
