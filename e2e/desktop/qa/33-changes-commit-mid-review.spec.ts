// Committing from the Changes tab while a review is open: the commit is the
// tab's own write, so the file list follows at once (no stale banner) and
// the pending comment stays. Runs on the alerting workspace's uncommitted
// edits (qa/launch.ts).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("changes: a commit mid-review lists what is left at once", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);

  // The first added line's gutter (not observable, so by place in the
  // 1400x900 window), then Left opens its comment box.
  await page.mouse.click(600, 277);
  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill("Keep this after the commit");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);
  await settle(page, 800);
  await capture(page, {
    name: "33-commit-review-01-before",
    title: "A review open on uncommitted files",
    expectations: [
      "Changes lists alerting.ts and levels.ts with a commit box above them",
      "The review bar says 1 comment pending; the comment card shows under its line",
    ],
  });

  await page.getByRole("textbox", { name: "Message" }).fill("feat: add alert levels");
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await expect(page.getByRole("status", { name: "Nothing to commit", exact: true })).toBeVisible({
    timeout: 15_000,
  });
  await settle(page, 1500);
  await expect(page.getByRole("button", { name: "Reload" })).toHaveCount(0);
  await capture(page, {
    name: "33-commit-review-02-after",
    title: "After the commit: the list follows, the review stays",
    expectations: [
      "Changes says 'Nothing to commit'; the committed files moved under Committed",
      "No amber 'changed since you started reviewing' banner and no Reload button",
      "The review bar still says 1 comment pending",
    ],
  });
});
